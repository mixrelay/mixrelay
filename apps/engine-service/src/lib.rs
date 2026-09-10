use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use parking_lot::{Mutex, RwLock};
mod recorder_control;
use mixrelay_audio_engine::{
    BlockProcessor, ExternalBinding, MeterBinding, PeakMeterBank, ProcessorConfigurationError,
    ProcessorExchange, ProcessorSubmitError, RenderPlan, RenderPlanError,
};
use mixrelay_audio_graph::{
    AUDIO_PORT_ID, AudioEndpoint, ChannelLayout, ChannelMapping, CompileContext, CompileLimits,
    EdgeId, EdgeSpec, ExecutionPlan, FROM_OS_PORT_ID, GainConfig, GraphCompiler, GraphDocument,
    NodeConfig, NodeId, NodeSpec, PortId, RenderFormat, StreamSpec, TO_OS_PORT_ID,
    VirtualEndpointId, VirtualEndpointSpec,
};
use mixrelay_audio_io::{
    HostAudioBackendError, VirtualEndpointBackend, VirtualEndpointCapabilities,
};
use mixrelay_control_protocol::{
    AppStatus, DriverState, ErrorCode, ExternalProcessorCatalog, GraphApplyRequest,
    GraphApplyResponse, GraphApplyState, GraphRuntimeStatus, MeterSnapshot, NodeMeterSnapshot,
    PROTOCOL_VERSION, ProcessorEditorStatus, ProcessorParameterUpdateResponse,
    ProcessorScanRequest, ProtocolError, SessionReconfigureRequest,
};
use mixrelay_control_server::ControlBackend;
use mixrelay_processor_host::{
    CatalogNodeResolver, ExternalProcessorFactory, ProcessorCatalog, ProcessorControlRegistry,
};

pub const DEFAULT_ENDPOINT_NODE_ID: &str = "default-virtual-io";

#[cfg(target_os = "macos")]
use mixrelay_macos_transport as _;

#[cfg(target_os = "macos")]
#[path = "macos/endpoint_registry.rs"]
pub mod endpoint_registry;

#[cfg(target_os = "macos")]
#[path = "macos/endpoint_service/mod.rs"]
pub mod endpoint_service;

mod control_maintenance;
#[cfg(test)]
mod controller_startup_tests;
#[cfg(test)]
mod controller_test_support;
pub mod device_worker;
mod persistence;
mod processor_timing;
pub use control_maintenance::ControlMaintenance;
pub mod catalog_execution;
mod endpoint_format;
#[cfg(test)]
mod endpoint_format_tests;
#[cfg(test)]
mod endpoint_persistence_tests;
#[cfg(test)]
mod endpoint_settings_tests;
mod execution_control;
mod graph_catalog;
#[cfg(test)]
mod graph_catalog_tests;
pub mod graph_composition;

pub mod render_demand;
pub mod resource_requirements;
pub mod session;
#[cfg(test)]
mod session_change_tests;
mod session_preferences;
#[cfg(test)]
mod virtual_source_gain_tests;
pub mod windows_transport_contract;

struct ContractOnlyVirtualEndpointBackend {
    capabilities: VirtualEndpointCapabilities,
}

/// Prepares operating-system resources that are coupled to one complete graph document.
///
/// Endpoint-only validation remains in [`VirtualEndpointBackend`]. This second boundary lets a
/// platform atomically stage endpoint transports and application-capture sessions together,
/// without teaching the portable graph or DSP crates about `PipeWire`, `WASAPI`, or Core Audio.
pub trait GraphResourceBackend: Send + Sync {
    /// Reads actual native identities for canonical hardware source/sink ownership.
    /// # Errors
    /// Rejects adapters without native device enumeration.
    fn device_inventory(
        &self,
    ) -> Result<Vec<mixrelay_audio_io::AudioDeviceDescriptor>, HostAudioBackendError> {
        Err(HostAudioBackendError::CapabilityUnavailable {
            capability: "native device identity resolution".into(),
        })
    }

    /// Resolves an application capture policy to one canonical zero-gain native source.
    /// # Errors
    /// Rejects selectors or capture policies that the adapter cannot fulfill exactly.
    fn resolve_application_capture(
        &self,
        _request: &mixrelay_audio_io::ApplicationCaptureRequest,
    ) -> Result<graph_composition::SharedSource, HostAudioBackendError> {
        Err(HostAudioBackendError::CapabilityUnavailable {
            capability: "application capture identity resolution".into(),
        })
    }
    /// Stages every native resource required by `document` on the control thread.
    ///
    /// # Errors
    ///
    /// Returns a host error before the prepared processor is published to the audio thread.
    fn reconcile(&self, document: &GraphDocument) -> Result<(), HostAudioBackendError>;

    /// Prepares format-dependent native transports off RT before native negotiation.
    /// Dropping the result must cancel only this staging, leaving active owners intact.
    /// Implementations must reserve their publication slot here so `commit` cannot fail.
    /// # Errors
    /// Rejects unsupported bindings or native preparation/capacity failures.
    fn prepare_format_change(
        &self,
        document: &GraphDocument,
        current: RenderFormat,
        requested: RenderFormat,
    ) -> Result<Box<dyn PreparedGraphFormat>, HostAudioBackendError> {
        if current != requested
            && document.nodes.iter().any(|node| {
                matches!(
                    node.config,
                    NodeConfig::ApplicationCapture(_)
                        | NodeConfig::AudioDeviceInput(_)
                        | NodeConfig::AudioDeviceOutput(_)
                )
            })
        {
            return Err(HostAudioBackendError::CapabilityUnavailable {
                capability: "format-changing capture/device transport".into(),
            });
        }
        Ok(Box::new(UnchangedGraphFormat))
    }
}

/// Reserved native format handoff. Created and dropped only on the control plane.
/// `commit` is called while DSP is suspended, immediately before its new plan publishes.
/// It must be infallible: all allocation, validation and mailbox reservation happens in
/// `prepare_format_change`. The native consumer must activate at the matching graph revision.
pub trait PreparedGraphFormat: Send {
    /// Apply checked native client settings after DSP suspension. An error must
    /// roll back changes owned by this token before returning to the controller.
    /// # Errors
    /// Rejects native settings that failed actual readback or rollback.
    fn configure_native(&mut self) -> Result<(), HostAudioBackendError> {
        Ok(())
    }
    fn commit(self: Box<Self>);
}

struct UnchangedGraphFormat;
impl PreparedGraphFormat for UnchangedGraphFormat {
    fn commit(self: Box<Self>) {}
}

/// Supplies host-driver health without coupling the control protocol to a platform module.
pub trait RuntimeStatusBackend: Send + Sync {
    fn status(&self, format: RenderFormat) -> AppStatus;

    /// Quiesces processing at an audio boundary before a device transaction.
    /// The controller resumes after publication or a verified rollback.
    /// # Errors
    /// Rejects unavailable suspension or an unacknowledged callback boundary.
    fn suspend(&self) -> Result<(), HostAudioBackendError> {
        Err(HostAudioBackendError::CapabilityUnavailable {
            capability: "audio processing suspension".into(),
        })
    }

    /// Latches a failed native rollback. Implementations exposing device setters
    /// must retain suspension until an explicit recovery establishes a valid plan.
    fn fail_reconfiguration(&self) {}

    /// Capability projection for a proposed format, not the currently active DSP format.
    fn capabilities_for_format(
        &self,
        format: RenderFormat,
    ) -> Option<mixrelay_audio_io::session::AudioSessionCapabilities> {
        self.status(format)
            .audio_session
            .map(|session| session.capabilities)
    }
    /// Reads a native rate only for an explicitly opted-in external-clock follower.
    /// Called on the control plane, never on the audio callback.
    fn external_sample_rate(&self) -> Option<u32> {
        None
    }
    /// Full own-endpoint observation. Legacy rate-only adapters retain their DSP block size.
    fn external_format(&self, current: RenderFormat) -> Option<RenderFormat> {
        self.external_sample_rate().map(|sample_rate| RenderFormat {
            sample_rate,
            ..current
        })
    }
    /// Adopts an already changed native rate without writing it back. Last-accepted format
    /// persistence may warn, but cannot veto adoption of an otherwise valid native format.
    /// # Errors
    /// Rejects unsupported followers or a native rate that changed during preparation.
    fn adopt_external_format(&self, _requested: RenderFormat) -> Result<(), HostAudioBackendError> {
        Err(HostAudioBackendError::CapabilityUnavailable {
            capability: "external format following".into(),
        })
    }
    /// Quiesces I/O and negotiates a native format on the control thread.
    /// # Errors
    /// Returns an error on unsupported or failed negotiation; the old format must be restored
    /// or audio must remain suspended. Publication happens before `resume` is called.
    fn reconfigure(
        &self,
        _current: RenderFormat,
        _requested: RenderFormat,
    ) -> Result<(), HostAudioBackendError> {
        Err(HostAudioBackendError::CapabilityUnavailable {
            capability: "live format reconfiguration".into(),
        })
    }
    fn resume(&self) {}
}

#[derive(Debug)]
pub struct FixedRuntimeStatus {
    driver_state: DriverState,
}

impl FixedRuntimeStatus {
    #[must_use]
    pub const fn new(driver_state: DriverState) -> Self {
        Self { driver_state }
    }
}

impl RuntimeStatusBackend for FixedRuntimeStatus {
    fn status(&self, format: RenderFormat) -> AppStatus {
        let ready = self.driver_state == DriverState::Ready;
        AppStatus {
            processor_timings: Vec::new(),
            recorder_error: None,
            protocol_version: PROTOCOL_VERSION,
            driver_state: self.driver_state,
            sample_rate: ready.then_some(format.sample_rate),
            buffer_frames: ready.then_some(format.block_frames),
            underruns: 0,
            overruns: 0,
            audio_session: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct NoopGraphResourceBackend;

impl GraphResourceBackend for NoopGraphResourceBackend {
    fn reconcile(&self, _document: &GraphDocument) -> Result<(), HostAudioBackendError> {
        Ok(())
    }
}

impl Default for ContractOnlyVirtualEndpointBackend {
    fn default() -> Self {
        Self {
            capabilities: VirtualEndpointCapabilities {
                max_render_channels: 64,
                max_capture_channels: 64,
                supported_sample_rates: vec![44_100, 48_000],
                supports_duplex: true,
                supports_dynamic_creation: false,
            },
        }
    }
}

impl VirtualEndpointBackend for ContractOnlyVirtualEndpointBackend {
    fn capabilities(&self) -> &VirtualEndpointCapabilities {
        &self.capabilities
    }

    fn prepare_graph_attachments(
        &self,
        _desired: &[VirtualEndpointSpec],
    ) -> Result<(), HostAudioBackendError> {
        Ok(())
    }
}

trait ProcessorFactory: Send + Sync {
    fn compiler(&self) -> GraphCompiler;
    fn prepare(
        &self,
        execution: &ExecutionPlan,
    ) -> Result<Box<BlockProcessor>, PrepareProcessorError>;
}

struct BuiltInProcessorFactory {
    recorders: mixrelay_audio_recorder::RecorderRegistry,
    catalog: ProcessorCatalog,
    hosted: ExternalProcessorFactory,
    meters: Arc<PeakMeterBank>,
    render_prepare: fn(&ExecutionPlan) -> Result<RenderPlan, RenderPlanError>,
}

impl Default for BuiltInProcessorFactory {
    fn default() -> Self {
        Self {
            recorders: mixrelay_audio_recorder::RecorderRegistry::default(),
            catalog: ProcessorCatalog::default(),
            hosted: ExternalProcessorFactory::new(ProcessorCatalog::default()),
            meters: default_peak_meter_bank(),
            render_prepare: RenderPlan::prepare,
        }
    }
}

impl ProcessorFactory for BuiltInProcessorFactory {
    fn compiler(&self) -> GraphCompiler {
        GraphCompiler::new(CatalogNodeResolver::new(self.catalog.clone()))
    }

    fn prepare(
        &self,
        execution: &ExecutionPlan,
    ) -> Result<Box<BlockProcessor>, PrepareProcessorError> {
        let mut processor = if execution
            .nodes()
            .iter()
            .any(|node| matches!(node.config(), NodeConfig::Provider(_)))
        {
            BlockProcessor::prepare_with_hosted_processors(
                execution,
                Arc::clone(&self.meters),
                &self.hosted,
            )
            .map_err(PrepareProcessorError::Render)?
        } else {
            let render = (self.render_prepare)(execution).map_err(PrepareProcessorError::Render)?;
            BlockProcessor::with_peak_meters(Arc::new(render), Arc::clone(&self.meters))
        };
        processor
            .attach_recorders(&self.recorders)
            .map_err(PrepareProcessorError::Recorder)?;
        Ok(Box::new(processor))
    }
}

#[derive(Debug)]
enum PrepareProcessorError {
    Render(RenderPlanError),
    Recorder(String),
}

pub struct EngineGraphController {
    recorder_cleanup_error: Mutex<Option<String>>,
    recorders: mixrelay_audio_recorder::RecorderRegistry,
    exchange: Arc<ProcessorExchange>,
    factory: Arc<dyn ProcessorFactory>,
    graph: RwLock<GraphDocument>,
    context: RwLock<CompileContext>,
    mutation: Mutex<()>,
    catalog: Mutex<Option<graph_catalog::CatalogStore>>,
    catalog_write_error: Mutex<Option<String>>,
    catalog_restore: Mutex<Option<graph_catalog::PendingRestore>>,
    required_endpoint_node_id: Option<NodeId>,
    endpoint_backend: Arc<dyn VirtualEndpointBackend>,
    meter_bindings: RwLock<BTreeMap<u64, Vec<MeterBinding>>>,
    execution_views: RwLock<BTreeMap<u64, execution_control::ExecutionView>>,
    meters: Arc<PeakMeterBank>,
    processor_catalog: ProcessorCatalog,
    processor_controls: ProcessorControlRegistry,
    graph_resources: Arc<dyn GraphResourceBackend>,
    runtime_status: Arc<dyn RuntimeStatusBackend>,
}

pub struct EngineGraphControllerConfig {
    pub format: RenderFormat,
    pub meters: Arc<PeakMeterBank>,
    pub initial_bindings: Vec<MeterBinding>,
    pub endpoint_backend: Arc<dyn VirtualEndpointBackend>,
    pub required_endpoint_node_id: Option<NodeId>,
    pub processor_catalog: ProcessorCatalog,
    pub graph_resources: Arc<dyn GraphResourceBackend>,
    pub runtime_status: Arc<dyn RuntimeStatusBackend>,
}

impl EngineGraphController {
    #[must_use]
    pub fn new(
        exchange: Arc<ProcessorExchange>,
        endpoint_node_id: impl Into<NodeId>,
        format: RenderFormat,
    ) -> Self {
        Self::new_with_peak_meters(
            exchange,
            endpoint_node_id,
            format,
            default_peak_meter_bank(),
            Vec::new(),
        )
    }

    #[must_use]
    pub fn new_with_peak_meters(
        exchange: Arc<ProcessorExchange>,
        endpoint_node_id: impl Into<NodeId>,
        format: RenderFormat,
        meters: Arc<PeakMeterBank>,
        initial_bindings: Vec<MeterBinding>,
    ) -> Self {
        Self::new_with_configuration(
            exchange,
            EngineGraphControllerConfig {
                format,
                meters,
                initial_bindings,
                endpoint_backend: Arc::new(ContractOnlyVirtualEndpointBackend::default()),
                required_endpoint_node_id: Some(endpoint_node_id.into()),
                processor_catalog: ProcessorCatalog::default(),
                graph_resources: Arc::new(NoopGraphResourceBackend),
                runtime_status: Arc::new(FixedRuntimeStatus::new(DriverState::Ready)),
            },
        )
    }

    #[must_use]
    pub fn new_with_virtual_endpoints(
        exchange: Arc<ProcessorExchange>,
        format: RenderFormat,
        endpoint_backend: Arc<dyn VirtualEndpointBackend>,
    ) -> Self {
        Self::new_with_configuration(
            exchange,
            EngineGraphControllerConfig {
                format,
                meters: default_peak_meter_bank(),
                initial_bindings: Vec::new(),
                endpoint_backend,
                required_endpoint_node_id: None,
                processor_catalog: ProcessorCatalog::default(),
                graph_resources: Arc::new(NoopGraphResourceBackend),
                runtime_status: Arc::new(FixedRuntimeStatus::new(DriverState::Ready)),
            },
        )
    }

    #[must_use]
    /// Publishes the document already compiled by a native adapter at startup.
    /// Capability flags do not imply that a default device node actually exists.
    ///
    /// # Panics
    /// Panics if the supplied document is not the active processor revision.
    pub fn new_with_initial_document(
        exchange: Arc<ProcessorExchange>,
        config: EngineGraphControllerConfig,
        document: GraphDocument,
    ) -> Self {
        assert_eq!(
            document.revision,
            exchange.revisions().active,
            "initial graph must match the active processor revision"
        );
        let EngineGraphControllerConfig {
            format,
            meters,
            initial_bindings,
            endpoint_backend,
            required_endpoint_node_id,
            processor_catalog,
            graph_resources,
            runtime_status,
        } = config;
        let active_revision = exchange.revisions().active;
        let mut meter_bindings = BTreeMap::new();
        meter_bindings.insert(active_revision, initial_bindings);
        let processor_controls = ProcessorControlRegistry::default();
        let recorders = mixrelay_audio_recorder::RecorderRegistry::default();
        Self {
            recorder_cleanup_error: Mutex::new(None),
            recorders: recorders.clone(),
            exchange,
            factory: Arc::new(BuiltInProcessorFactory {
                recorders,
                catalog: processor_catalog.clone(),
                hosted: ExternalProcessorFactory::with_controls(
                    processor_catalog.clone(),
                    processor_controls.clone(),
                ),
                meters: Arc::clone(&meters),
                render_prepare: RenderPlan::prepare,
            }),
            graph: RwLock::new(document),
            context: RwLock::new(CompileContext {
                format,
                limits: CompileLimits::default(),
            }),
            mutation: Mutex::new(()),
            catalog: Mutex::new(None),
            catalog_write_error: Mutex::new(None),
            catalog_restore: Mutex::new(None),
            required_endpoint_node_id,
            endpoint_backend,
            meter_bindings: RwLock::new(meter_bindings),
            execution_views: RwLock::new(BTreeMap::new()),
            meters,
            processor_catalog,
            processor_controls,
            graph_resources,
            runtime_status,
        }
    }

    /// Constructs a controller with the built-in session document selected from
    /// the configured capabilities. Native startup with an already prepared graph
    /// must use `new_with_initial_document` instead.
    #[must_use]
    pub fn new_with_configuration(
        exchange: Arc<ProcessorExchange>,
        config: EngineGraphControllerConfig,
    ) -> Self {
        let revision = exchange.revisions().active;
        let document = if config
            .runtime_status
            .status(config.format)
            .audio_session
            .is_some_and(|session| !session.capabilities.virtual_endpoint)
        {
            GraphDocument::new(revision, vec![], vec![])
        } else {
            default_session_document(revision, config.format)
        };
        Self::new_with_initial_document(exchange, config, document)
    }

    #[cfg(test)]
    fn prepare_and_validate(
        &self,
        document: &GraphDocument,
        context: CompileContext,
    ) -> Result<execution_control::PreparedExecution, ProtocolError> {
        let catalog = self.catalog_candidate(document);
        self.prepare_execution(document, catalog.as_ref(), context)
    }

    fn validate_required_endpoint(&self, document: &GraphDocument) -> Result<(), ProtocolError> {
        if let Some(required) = &self.required_endpoint_node_id
            && !document.nodes.iter().any(|node| {
                node.id == *required && matches!(node.config, NodeConfig::VirtualEndpoint(_))
            })
        {
            return Err(ProtocolError {
                code: ErrorCode::GraphBindingUnsupported,
                parameters: serde_json::json!({
                    "requiredEndpointNodeId": required.as_str(),
                }),
            });
        }
        Ok(())
    }

    fn validate_session_bindings(
        &self,
        document: &GraphDocument,
        format: RenderFormat,
    ) -> Result<(), ProtocolError> {
        let session = self.runtime_status.capabilities_for_format(format);
        for node in &document.nodes {
            let reason = match &node.config {
                NodeConfig::ApplicationCapture(_)
                    if session.as_ref().is_some_and(|s| !s.application_capture) =>
                {
                    Some(
                        "application capture is not supported by the active adapter at this session format",
                    )
                }
                NodeConfig::VirtualEndpoint(endpoint)
                    if self
                        .endpoint_backend
                        .sample_rate(&endpoint.id)
                        .is_ok_and(|rate| endpoint.default_sample_rate != rate) =>
                {
                    Some("endpoint sample rate differs from its available native clock")
                }
                NodeConfig::AudioDeviceInput(_)
                    if session.as_ref().is_some_and(|s| !s.hardware_input) =>
                {
                    Some("hardware input is not implemented by this audio adapter")
                }
                NodeConfig::AudioDeviceOutput(_)
                    if session.as_ref().is_some_and(|s| !s.hardware_output) =>
                {
                    Some("hardware output is not implemented by this audio adapter")
                }
                NodeConfig::VirtualEndpoint(_)
                    if session.as_ref().is_some_and(|s| !s.virtual_endpoint) =>
                {
                    Some("the virtual endpoint driver is unavailable on this audio adapter")
                }
                _ => None,
            };
            if let Some(reason) = reason {
                return Err(ProtocolError {
                    code: ErrorCode::GraphBindingUnsupported,
                    parameters: serde_json::json!({ "nodeId": node.id.as_str(), "reason": reason,
                        "sampleRate": format.sample_rate, "blockFrames": format.block_frames }),
                });
            }
        }
        Ok(())
    }
}

impl ControlBackend for EngineGraphController {
    fn recorder_status(
        &self,
        graph_id: &str,
        node_id: &str,
    ) -> Result<mixrelay_control_protocol::RecorderStatus, ProtocolError> {
        let _mutation = self.mutation.lock();
        let id = self.recorder_session_id(graph_id, node_id)?;
        Ok(self.recorders.status(id.as_str()))
    }

    fn control_recorder(
        &self,
        graph_id: &str,
        node_id: &str,
        request: mixrelay_control_protocol::RecorderRequest,
    ) -> Result<mixrelay_control_protocol::RecorderStatus, ProtocolError> {
        let _mutation = self.mutation.lock();
        let id = self.recorder_session_id(graph_id, node_id)?;
        self.recorders
            .command(id.as_str(), request)
            .map_err(recorder_control::recorder_error)
    }
    fn application_capture_manifest(
        &self,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        self.current_capture_manifest()
    }

    fn preview_application_capture(
        &self,
        request: GraphApplyRequest,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        self.preview_capture_manifest(request)
    }
    fn reconfigure_virtual_endpoint(
        &self,
        request: mixrelay_control_protocol::VirtualEndpointFormatRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        self.change_endpoint_format(&request)
    }
    fn graph_layout(
        &self,
        id: &str,
    ) -> Result<mixrelay_control_protocol::GraphLayoutResponse, ProtocolError> {
        self.read_graph_layout(id)
    }

    fn update_graph_layout(
        &self,
        id: &str,
        request: mixrelay_control_protocol::GraphLayoutRequest,
    ) -> Result<mixrelay_control_protocol::GraphLayoutResponse, ProtocolError> {
        self.write_graph_layout(id, request)
    }
    fn graph_catalog(
        &self,
    ) -> Result<mixrelay_control_protocol::GraphCatalogResponse, ProtocolError> {
        self.read_catalog()
    }

    fn graph_catalog_document(&self, id: &str) -> Result<GraphDocument, ProtocolError> {
        self.read_catalog_document(id)
    }

    fn mutate_graph_catalog(
        &self,
        request: mixrelay_control_protocol::GraphCatalogRequest,
    ) -> Result<mixrelay_control_protocol::GraphCatalogResponse, ProtocolError> {
        self.mutate_catalog(request)
    }
    fn reconfigure_session(
        &self,
        request: SessionReconfigureRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        self.change_session(request)
    }
    fn status(&self) -> AppStatus {
        let format = self.context.read().format;
        let mut status = self.runtime_status.status(format);
        status.processor_timings = self.processor_timings();
        status
            .recorder_error
            .clone_from(&self.recorder_cleanup_error.lock());
        if let Some(session) = &mut status.audio_session {
            session.virtual_endpoint_settings = self.endpoint_settings();
        }
        status
    }

    fn graph_status(&self) -> GraphRuntimeStatus {
        let revisions = self.exchange.revisions();
        GraphRuntimeStatus {
            protocol_version: PROTOCOL_VERSION,
            active_revision: revisions.active,
            submitted_revision: revisions.submitted,
        }
    }

    fn graph(&self) -> GraphDocument {
        self.graph.read().clone()
    }

    fn meters(&self) -> MeterSnapshot {
        let _mutation = self.mutation.lock();
        let graph_id = self.view_graph_id();
        let revision = self.exchange.revisions().active;
        let bindings = self.meter_bindings.read();
        let views = self.execution_views.read();
        let view = views.get(&revision);
        let nodes = bindings
            .get(&revision)
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, binding)| {
                // Drain hidden meters too: selecting a graph must not expose old peaks.
                let peaks = self.meters.take_peaks(revision, index, binding.channels());
                let origin = view?.origins.get(binding.node_id())?;
                (origin.graph_id == graph_id).then(|| NodeMeterSnapshot {
                    node_id: origin.node_id.as_str().to_owned(),
                    port_id: origin
                        .meter_port
                        .as_ref()
                        .unwrap_or(binding.port_id())
                        .as_str()
                        .to_owned(),
                    peaks,
                })
            })
            .collect();
        MeterSnapshot {
            protocol_version: PROTOCOL_VERSION,
            revision,
            nodes,
        }
    }

    fn processors(&self) -> ExternalProcessorCatalog {
        self.processor_catalog.protocol_catalog()
    }

    fn rescan_processors(
        &self,
        request: ProcessorScanRequest,
    ) -> Result<ExternalProcessorCatalog, ProtocolError> {
        let paths = request
            .paths
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect::<Vec<_>>();
        if let Some(path) = paths.iter().find(|path| !path.is_dir()) {
            return Err(ProtocolError {
                code: ErrorCode::ProcessorUnavailable,
                parameters: serde_json::json!({
                    "reason": "processor scan path is not a directory",
                    "path": path,
                }),
            });
        }
        self.processor_catalog.rescan(&paths);
        Ok(self.processor_catalog.protocol_catalog())
    }

    fn set_processor_parameter(
        &self,
        node_id: &str,
        parameter_id: u32,
        value: f64,
    ) -> Result<ProcessorParameterUpdateResponse, ProtocolError> {
        self.apply_processor_parameter_update(
            node_id,
            parameter_id,
            value,
            |provider, runtime_id| {
                if !self
                    .processor_catalog
                    .parameter_is_writable(provider.type_id.as_str(), parameter_id)
                {
                    return Err(processor_parameter_error(
                        node_id,
                        parameter_id,
                        "processor parameter is unavailable or read-only",
                    ));
                }
                self.processor_controls
                    .set_parameter(runtime_id, parameter_id, value)
                    .map_err(|reason| processor_parameter_error(node_id, parameter_id, &reason))
            },
        )
    }

    fn processor_editor_status(
        &self,
        node_id: &str,
    ) -> Result<ProcessorEditorStatus, ProtocolError> {
        let _mutation = self.mutation.lock();
        let runtime_id = self.external_processor_node(node_id)?;
        let mut status = self
            .processor_controls
            .editor_status(&runtime_id)
            .map_err(|reason| processor_editor_error(node_id, &reason))?;
        node_id.clone_into(&mut status.node_id);
        Ok(status)
    }

    fn set_processor_editor(
        &self,
        node_id: &str,
        open: bool,
    ) -> Result<ProcessorEditorStatus, ProtocolError> {
        let _mutation = self.mutation.lock();
        let runtime_id = self.external_processor_node(node_id)?;
        if !open {
            self.processor_controls
                .cancel_editor_gesture(&runtime_id)
                .map_err(|reason| processor_editor_error(node_id, &reason))?;
            // Preserve vendor edits while the instance is still available. A failed
            // durable checkpoint must remain retryable, not silently close the editor.
            self.checkpoint_processor_state_locked(Some(node_id))?;
        }
        let mut status = self
            .processor_controls
            .set_editor_open(&runtime_id, open)
            .map_err(|reason| processor_editor_error(node_id, &reason))?;
        node_id.clone_into(&mut status.node_id);
        Ok(status)
    }

    fn processor_editor_frame(
        &self,
        node_id: &str,
    ) -> Result<mixrelay_control_protocol::ProcessorEditorFrame, ProtocolError> {
        let runtime_id = {
            let _mutation = self.mutation.lock();
            self.external_processor_node(node_id)?
        };
        self.processor_controls
            .editor_frame(&runtime_id)
            .map_err(|reason| processor_editor_error(node_id, &reason))
    }

    fn processor_editor_input(
        &self,
        node_id: &str,
        input: mixrelay_control_protocol::ProcessorEditorInput,
    ) -> Result<(), ProtocolError> {
        let runtime_id = {
            let _mutation = self.mutation.lock();
            self.external_processor_node(node_id)?
        };
        self.processor_controls
            .editor_input(&runtime_id, &input)
            .map_err(|reason| processor_editor_error(node_id, &reason))
    }

    fn apply_graph(&self, document: GraphDocument) -> Result<GraphApplyResponse, ProtocolError> {
        self.apply_graph_request(document.into())
    }

    fn apply_graph_request(
        &self,
        request: GraphApplyRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        let _mutation = self.mutation.lock();
        let mut document = request.document;
        self.preserve_live_processor_state(&mut document, &request.processor_parameter_changes)?;
        let catalog = self.catalog_candidate(&document);
        self.apply_graph_locked(document, catalog)
    }
}

impl EngineGraphController {
    /// Native publication precedes durable control-plane bookkeeping. Saving an accepted
    /// parameter never prepares a graph, increments its revision, or replaces a live worker.
    fn apply_processor_parameter_update(
        &self,
        node_id: &str,
        parameter_id: u32,
        value: f64,
        publish: impl FnOnce(
            &mixrelay_audio_graph::ProviderNodeConfig,
            &NodeId,
        ) -> Result<(), ProtocolError>,
    ) -> Result<ProcessorParameterUpdateResponse, ProtocolError> {
        let _mutation = self.mutation.lock();
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(processor_parameter_error(
                node_id,
                parameter_id,
                "parameter value must be normalized between 0 and 1",
            ));
        }
        let runtime_id =
            graph_composition::runtime_node_id(&self.view_graph_id(), &NodeId::from(node_id));
        let mut graph = self.graph.write();
        let provider = graph
            .nodes
            .iter_mut()
            .find(|node| node.id.as_str() == node_id)
            .and_then(|node| match &mut node.config {
                NodeConfig::Provider(provider) => Some(provider),
                _ => None,
            })
            .ok_or_else(|| {
                processor_parameter_error(node_id, parameter_id, "processor node does not exist")
            })?;
        publish(provider, &runtime_id)?;
        persist_processor_parameter(provider, parameter_id, value);
        let document = graph.clone();
        drop(graph);
        let persistence_error = self.persist_parameter_document(&document);
        Ok(ProcessorParameterUpdateResponse {
            protocol_version: PROTOCOL_VERSION,
            node_id: node_id.to_owned(),
            parameter_id,
            value,
            persistence_error,
        })
    }

    fn apply_graph_locked(
        &self,
        mut document: GraphDocument,
        mut catalog: Option<graph_catalog::Catalog>,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        self.validate_recorder_channel_change(&document)?;
        if let Some(candidate) = &mut catalog {
            self.reconcile_catalog_devices(candidate)?;
            self.preserve_catalog_provider_state(candidate)?;
            document = candidate.active_document().clone();
        }
        let revisions = self.exchange.revisions();
        if document.revision <= revisions.submitted {
            return Err(ProtocolError {
                code: ErrorCode::GraphRevisionStale,
                parameters: serde_json::json!({
                    "activeRevision": revisions.active,
                    "submittedRevision": revisions.submitted,
                    "proposedRevision": document.revision,
                }),
            });
        }
        let revision = document.revision;
        let execution_control::PreparedExecution {
            processor,
            endpoints,
            document: runtime_document,
            view,
        } = self.prepare_execution(&document, catalog.as_ref(), *self.context.read())?;
        let meter_bindings = processor.plan().meter_bindings().to_vec();
        let staged_catalog = self.stage_catalog(catalog.as_ref())?;
        self.exchange
            .submit_configured(processor, || {
                self.endpoint_backend
                    .prepare_graph_attachments(&endpoints)?;
                if let Err(error) = self.graph_resources.reconcile(&runtime_document) {
                    self.endpoint_backend.cancel_prepared_graph_attachments();
                    return Err(error);
                }
                self.finish_catalog_write(staged_catalog);
                Ok(())
            })
            .map_err(configuration_error)?;
        let active_revision = self.exchange.revisions().active;
        let mut bindings = self.meter_bindings.write();
        bindings.retain(|existing, _| *existing == active_revision);
        bindings.insert(revision, meter_bindings);
        drop(bindings);
        let mut views = self.execution_views.write();
        views.retain(|existing, _| *existing == active_revision);
        views.insert(revision, view);
        drop(views);
        *self.graph.write() = document;
        self.commit_catalog_memory(catalog);
        *self.catalog_restore.lock() = None;
        Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision,
            state: GraphApplyState::Pending,
            persistence_error: self.catalog_write_error.lock().clone(),
        })
    }
}

impl EngineGraphController {
    /// Reconciles an external OS/DAW rate change on a non-real-time control thread.
    /// No native setter is invoked in this path. A pending graph or
    /// another mutation defers reconciliation until the monitor's next observation.
    /// # Errors
    /// Reports unsupported formats/transports and native or preset preparation failures.
    pub fn reconcile_observed_device_rate(
        &self,
    ) -> Result<Option<GraphApplyResponse>, ProtocolError> {
        let Some(_mutation) = self.mutation.try_lock() else {
            return Ok(None);
        };
        let current = self.context.read().format;
        let Some(observed) = self.runtime_status.external_format(current) else {
            return Ok(None);
        };
        let revisions = self.exchange.revisions();
        if revisions.active != revisions.submitted {
            return Ok(None);
        }
        if observed == current {
            return self.reconcile_endpoint_clock_locked();
        }
        self.change_session_locked(
            SessionReconfigureRequest {
                expected_revision: revisions.submitted,
                format: mixrelay_audio_io::session::SessionFormat {
                    sample_rate: observed.sample_rate,
                    block_frames: observed.block_frames,
                },
            },
            SessionChangeOrigin::Observed,
            SessionPauseOwner::SessionChange,
        )
        .map(Some)
    }

    fn change_session(
        &self,
        request: SessionReconfigureRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        let _mutation = self.mutation.lock();
        self.change_session_locked(
            request,
            SessionChangeOrigin::User,
            SessionPauseOwner::SessionChange,
        )
    }

    fn reconcile_endpoint_clock_locked(&self) -> Result<Option<GraphApplyResponse>, ProtocolError> {
        let mut document = self.graph.read().clone();
        let mut catalog = self.catalog_candidate(&document);
        if !self.observe_endpoint_catalog(&mut document, &mut catalog)? {
            return Ok(None);
        }
        document.revision = document
            .revision
            .checked_add(1)
            .ok_or_else(|| session_error("revision overflow"))?;
        self.preserve_live_processor_state(&mut document, &[])?;
        if let Some(candidate) = &mut catalog {
            candidate.update_document(document.clone());
        }
        self.apply_graph_locked(document, catalog).map(Some)
    }

    fn change_session_locked(
        &self,
        request: SessionReconfigureRequest,
        origin: SessionChangeOrigin,
        pause_owner: SessionPauseOwner,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        let current = *self.context.read();
        let revisions = self.require_active_revision(request.expected_revision)?;
        self.validate_session_request(request, current.format, origin)?;
        if request.format.sample_rate == current.format.sample_rate
            && request.format.block_frames == current.format.block_frames
        {
            return self.accept_unchanged_session(
                current.format,
                revisions.active,
                origin,
                pause_owner,
            );
        }
        let mut document = self.graph.read().clone();
        document.revision = revisions
            .submitted
            .checked_add(1)
            .ok_or_else(|| session_error("revision overflow"))?;
        let next = CompileContext {
            format: RenderFormat {
                sample_rate: request.format.sample_rate,
                block_frames: request.format.block_frames,
            },
            ..current
        };
        self.preserve_live_processor_state(&mut document, &[])?;
        let mut catalog = self.observed_catalog_candidate(&mut document)?;
        if let Some(candidate) = &mut catalog {
            self.preserve_catalog_provider_state(candidate)?;
        }
        let execution_control::PreparedExecution {
            processor,
            document: runtime_document,
            view,
            ..
        } = self.prepare_execution(&document, catalog.as_ref(), next)?;
        let mut resources = self
            .graph_resources
            .prepare_format_change(&runtime_document, current.format, next.format)
            .map_err(|error| session_error(&error.to_string()))?;
        let bindings = processor.plan().meter_bindings().to_vec();
        let staged_catalog = self.stage_session_catalog(catalog.as_ref(), origin)?;
        let result = self.exchange.submit_configured(processor, || {
            self.configure_session_native(current.format, next.format, origin, resources.as_mut())?;
            resources.commit();
            self.finish_catalog_write(staged_catalog);
            Ok::<(), HostAudioBackendError>(())
        });
        if let Err(error) = result {
            self.resume_session(pause_owner);
            return Err(session_error(&error.to_string()));
        }
        *self.context.write() = next;
        let mut meters = self.meter_bindings.write();
        meters.retain(|revision, _| *revision == revisions.active);
        meters.insert(document.revision, bindings);
        drop(meters);
        let mut views = self.execution_views.write();
        views.retain(|revision, _| *revision == revisions.active);
        views.insert(document.revision, view);
        drop(views);
        let revision = document.revision;
        *self.graph.write() = document;
        self.commit_catalog_memory(catalog);
        // Resume only after the prepared plan is published; the worker activates it before
        // processing another block. Active format reporting belongs to that worker.
        self.resume_session(pause_owner);
        Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision,
            state: GraphApplyState::Pending,
            persistence_error: self.catalog_write_error.lock().clone(),
        })
    }

    /// Device/session mutations share the same settled revision precondition.
    /// The caller holds the mutation lock throughout validation and publication.
    fn require_active_revision(
        &self,
        expected: u64,
    ) -> Result<mixrelay_audio_engine::ProcessorRevisions, ProtocolError> {
        let revisions = self.exchange.revisions();
        if expected != revisions.submitted || revisions.active != revisions.submitted {
            return Err(ProtocolError {
                code: ErrorCode::GraphRevisionStale,
                parameters: serde_json::json!({
                    "activeRevision": revisions.active,
                    "submittedRevision": revisions.submitted,
                }),
            });
        }
        Ok(revisions)
    }

    fn configure_session_native(
        &self,
        current: RenderFormat,
        next: RenderFormat,
        origin: SessionChangeOrigin,
        resources: &mut dyn PreparedGraphFormat,
    ) -> Result<(), HostAudioBackendError> {
        if matches!(origin, SessionChangeOrigin::Observed) {
            return self.runtime_status.adopt_external_format(next);
        }
        self.runtime_status.reconfigure(current, next)?;
        if let Err(error) = resources.configure_native() {
            self.runtime_status
                .reconfigure(next, current)
                .map_err(|rollback| HostAudioBackendError::BackendFailure {
                    message: format!("{error}; session rollback failed: {rollback}"),
                })?;
            return Err(error);
        }
        Ok(())
    }

    fn accept_unchanged_session(
        &self,
        format: RenderFormat,
        revision: u64,
        origin: SessionChangeOrigin,
        pause_owner: SessionPauseOwner,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        // Saving/recovering the same format must not recreate plug-ins or invalidate history.
        if origin == SessionChangeOrigin::User {
            let result = self.runtime_status.reconfigure(format, format);
            self.resume_session(pause_owner);
            result.map_err(|error| session_error(&error.to_string()))?;
        }
        Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision,
            state: GraphApplyState::Active,
            persistence_error: self.catalog_write_error.lock().clone(),
        })
    }

    fn resume_session(&self, pause_owner: SessionPauseOwner) {
        // An enclosing endpoint negotiation owns its suspension through native
        // rollback acknowledgement. An inner adoption cannot release that pause.
        if pause_owner == SessionPauseOwner::SessionChange {
            self.runtime_status.resume();
        }
    }

    fn stage_session_catalog(
        &self,
        catalog: Option<&graph_catalog::Catalog>,
        origin: SessionChangeOrigin,
    ) -> Result<Option<persistence::StagedWrite>, ProtocolError> {
        match self.stage_catalog(catalog) {
            Ok(staged) => Ok(staged),
            Err(error) if origin == SessionChangeOrigin::Observed => {
                *self.catalog_write_error.lock() =
                    Some(format!("external format catalog save failed: {error:?}"));
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    fn validate_session_request(
        &self,
        request: SessionReconfigureRequest,
        current: RenderFormat,
        origin: SessionChangeOrigin,
    ) -> Result<(), ProtocolError> {
        let supported = self
            .runtime_status
            .status(current)
            .audio_session
            .is_some_and(|session| {
                (origin == SessionChangeOrigin::Observed
                    || session.capabilities.can_reconfigure_live)
                    && session
                        .capabilities
                        .supported_sample_rates
                        .contains(&request.format.sample_rate)
                    && session
                        .capabilities
                        .supported_block_frames
                        .contains(&request.format.block_frames)
            });
        if !supported {
            return Err(ProtocolError {
                code: ErrorCode::AudioSessionUnsupported,
                parameters: serde_json::json!({"reason": "requested session format is unavailable"}),
            });
        }
        // Native transport format changes are separately opt-in through a prepared
        // resource transaction; a broad OS capability does not bypass that boundary.
        Ok(())
    }

    fn external_processor_node(&self, node_id: &str) -> Result<NodeId, ProtocolError> {
        let node_id = NodeId::from(node_id);
        let runtime_id = graph_composition::runtime_node_id(&self.view_graph_id(), &node_id);
        self.graph
            .read()
            .nodes
            .iter()
            .any(|node| node.id == node_id && matches!(node.config, NodeConfig::Provider(_)))
            .then_some(runtime_id)
            .ok_or_else(|| {
                processor_editor_error(node_id.as_str(), "processor node does not exist")
            })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionChangeOrigin {
    User,
    Observed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionPauseOwner {
    SessionChange,
    EndpointChange,
}

fn session_error(reason: &str) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::AudioSessionReconfigureFailed,
        parameters: serde_json::json!({"reason": reason}),
    }
}

fn processor_parameter_error(node_id: &str, parameter_id: u32, reason: &str) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::ProcessorUnavailable,
        parameters: serde_json::json!({
            "nodeId": node_id,
            "parameterId": parameter_id,
            "reason": reason,
        }),
    }
}

fn processor_editor_error(node_id: &str, reason: &str) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::ProcessorUnavailable,
        parameters: serde_json::json!({ "nodeId": node_id, "reason": reason }),
    }
}

fn persist_processor_parameter(
    provider: &mut mixrelay_audio_graph::ProviderNodeConfig,
    parameter_id: u32,
    value: f64,
) {
    if !provider.parameters.is_object() {
        provider.parameters = serde_json::json!({});
    }
    let parameters = provider
        .parameters
        .as_object_mut()
        .expect("processor parameters were normalized to an object");
    parameters
        .entry("version")
        .or_insert_with(|| serde_json::json!(1));
    let values = parameters
        .entry("parameterValues")
        .or_insert_with(|| serde_json::json!({}));
    if !values.is_object() {
        *values = serde_json::json!({});
    }
    values
        .as_object_mut()
        .expect("parameter values were normalized to an object")
        .insert(parameter_id.to_string(), serde_json::json!(value));
}

#[must_use]
pub fn default_peak_meter_bank() -> Arc<PeakMeterBank> {
    let limits = CompileLimits::default();
    Arc::new(PeakMeterBank::new(
        limits.max_nodes.saturating_add(1),
        usize::from(limits.max_channels_per_port),
    ))
}

fn virtual_endpoint_specs(
    document: &GraphDocument,
    capabilities: &VirtualEndpointCapabilities,
) -> Result<Vec<(NodeId, VirtualEndpointSpec)>, ProtocolError> {
    let mut endpoint_ids = BTreeSet::new();
    let mut endpoints = Vec::new();
    for node in &document.nodes {
        let NodeConfig::VirtualEndpointTransport(endpoint) = &node.config else {
            continue;
        };
        if !endpoint_ids.insert(endpoint.id.clone()) {
            return Err(ProtocolError {
                code: ErrorCode::GraphBindingUnsupported,
                parameters: serde_json::json!({
                    "endpointId": endpoint.id.as_str(),
                    "reason": "virtual endpoint IDs must be unique",
                }),
            });
        }
        endpoint
            .validate(capabilities)
            .map_err(|error| ProtocolError {
                code: ErrorCode::GraphBindingUnsupported,
                parameters: serde_json::json!({
                    "endpointId": endpoint.id.as_str(),
                    "reason": error.to_string(),
                }),
            })?;
        endpoints.push((node.id.clone(), endpoint.clone()));
    }
    Ok(endpoints)
}

fn validate_endpoint_bindings(
    plan: &RenderPlan,
    endpoints: &[(NodeId, VirtualEndpointSpec)],
) -> Result<(), ProtocolError> {
    let mut missing = Vec::new();
    for (node_id, endpoint) in endpoints {
        if let Some(stream) = endpoint.render
            && !binding_exists(
                plan.source_bindings(),
                node_id,
                &endpoint.id,
                FROM_OS_PORT_ID,
                stream.channels,
            )
        {
            missing.push(serde_json::json!({
                "nodeId": node_id.as_str(),
                "portId": FROM_OS_PORT_ID,
                "channels": stream.channels,
            }));
        }
        if let Some(stream) = endpoint.capture
            && !binding_exists(
                plan.sink_bindings(),
                node_id,
                &endpoint.id,
                TO_OS_PORT_ID,
                stream.channels,
            )
        {
            missing.push(serde_json::json!({
                "nodeId": node_id.as_str(),
                "portId": TO_OS_PORT_ID,
                "channels": stream.channels,
            }));
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(ProtocolError {
        code: ErrorCode::GraphBindingUnsupported,
        parameters: serde_json::json!({
            "missing": missing,
            "sources": binding_values(plan.source_bindings()),
            "sinks": binding_values(plan.sink_bindings()),
        }),
    })
}

fn binding_exists(
    bindings: &[ExternalBinding],
    node_id: &NodeId,
    endpoint_id: &VirtualEndpointId,
    port_id: &str,
    channels: u16,
) -> bool {
    bindings.iter().any(|binding| {
        binding.node_id() == node_id
            && binding.virtual_endpoint_id() == Some(endpoint_id)
            && binding.port_id().as_str() == port_id
            && binding.channels() == channels
    })
}

fn binding_values(bindings: &[ExternalBinding]) -> Vec<serde_json::Value> {
    bindings
        .iter()
        .map(|binding| {
            serde_json::json!({
                "nodeId": binding.node_id().as_str(),
                "portId": binding.port_id().as_str(),
                "channels": binding.channels(),
            })
        })
        .collect()
}

fn prepare_error(error: PrepareProcessorError) -> ProtocolError {
    match error {
        PrepareProcessorError::Recorder(reason) => recorder_control::recorder_error(reason),
        PrepareProcessorError::Render(error) => ProtocolError {
            code: ErrorCode::GraphInvalid,
            parameters: serde_json::json!({ "renderError": error.to_string() }),
        },
    }
}

fn exchange_error(error: ProcessorSubmitError) -> ProtocolError {
    let ProcessorSubmitError::Busy { proposed_revision } = error;
    ProtocolError {
        code: ErrorCode::GraphExchangeBusy,
        parameters: serde_json::json!({ "proposedRevision": proposed_revision }),
    }
}

fn configuration_error(error: ProcessorConfigurationError<HostAudioBackendError>) -> ProtocolError {
    match error {
        ProcessorConfigurationError::Busy { proposed_revision } => {
            exchange_error(ProcessorSubmitError::Busy { proposed_revision })
        }
        ProcessorConfigurationError::Configuration(error) => ProtocolError {
            code: ErrorCode::AudioDeviceDisconnected,
            parameters: serde_json::json!({ "reason": error.to_string() }),
        },
    }
}

#[must_use]
pub fn default_session_document(revision: u64, format: RenderFormat) -> GraphDocument {
    let mut document = default_duplex_document(revision, None);
    for node in &mut document.nodes {
        if let NodeConfig::VirtualEndpoint(endpoint) = &mut node.config {
            endpoint.default_sample_rate = format.sample_rate;
            endpoint.supported_sample_rates = vec![format.sample_rate];
        }
    }
    document
}

#[must_use]
pub fn default_duplex_document(revision: u64, gain_db: Option<f32>) -> GraphDocument {
    let endpoint_id = NodeId::from(DEFAULT_ENDPOINT_NODE_ID);
    let endpoint = NodeSpec {
        id: endpoint_id.clone(),
        label: Some("MixRelay Virtual I/O".into()),
        config: NodeConfig::VirtualEndpoint(
            VirtualEndpointSpec {
                id: VirtualEndpointId::from(DEFAULT_ENDPOINT_NODE_ID),
                display_name: "MixRelay Virtual I/O".into(),
                render: Some(stereo_stream()),
                capture: Some(stereo_stream()),
                supported_sample_rates: vec![48_000],
                default_sample_rate: 48_000,
            }
            .into(),
        ),
    };
    match gain_db {
        None => GraphDocument::new(
            revision,
            vec![endpoint],
            (0..2)
                .map(|channel| {
                    channel_edge(
                        &format!("default-pass-through-{}", channel + 1),
                        channel_endpoint(DEFAULT_ENDPOINT_NODE_ID, FROM_OS_PORT_ID, channel),
                        channel_endpoint(DEFAULT_ENDPOINT_NODE_ID, TO_OS_PORT_ID, channel),
                    )
                })
                .collect(),
        ),
        Some(gain_db) => GraphDocument::new(
            revision,
            vec![
                endpoint,
                NodeSpec {
                    id: NodeId::from("output-gain"),
                    label: Some("Output gain".into()),
                    config: NodeConfig::Gain(GainConfig {
                        channels: 2,
                        gain_db,
                    }),
                },
            ],
            (0..2)
                .flat_map(|channel| {
                    [
                        channel_edge(
                            &format!("from-os-to-gain-{}", channel + 1),
                            channel_endpoint(DEFAULT_ENDPOINT_NODE_ID, FROM_OS_PORT_ID, channel),
                            channel_endpoint("output-gain", AUDIO_PORT_ID, channel),
                        ),
                        channel_edge(
                            &format!("gain-to-os-{}", channel + 1),
                            channel_endpoint("output-gain", AUDIO_PORT_ID, channel),
                            channel_endpoint(DEFAULT_ENDPOINT_NODE_ID, TO_OS_PORT_ID, channel),
                        ),
                    ]
                })
                .collect(),
        ),
    }
}

const fn stereo_stream() -> StreamSpec {
    StreamSpec {
        channels: 2,
        channel_layout: ChannelLayout::Stereo,
    }
}

fn audio_endpoint(node_id: &str, port_id: &str) -> AudioEndpoint {
    AudioEndpoint {
        node_id: NodeId::from(node_id),
        port_id: PortId::from(port_id),
    }
}

#[derive(Debug, Clone, Copy)]
struct ChannelEndpoint<'a> {
    node_id: &'a str,
    port_id: &'a str,
    channel: u16,
}

const fn channel_endpoint<'a>(
    node_id: &'a str,
    port_id: &'a str,
    channel: u16,
) -> ChannelEndpoint<'a> {
    ChannelEndpoint {
        node_id,
        port_id,
        channel,
    }
}

fn channel_edge(
    id: &str,
    source: ChannelEndpoint<'_>,
    destination: ChannelEndpoint<'_>,
) -> EdgeSpec {
    EdgeSpec {
        id: EdgeId::from(id),
        source: audio_endpoint(source.node_id, source.port_id),
        destination: audio_endpoint(destination.node_id, destination.port_id),
        channel_mapping: ChannelMapping::Explicit {
            routes: vec![mixrelay_audio_graph::ChannelRoute {
                source_channel: source.channel,
                destination_channel: destination.channel,
            }],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_empty_startup_does_not_invent_a_default_endpoint() {
        let controller = EngineGraphController::new_with_initial_document(
            Arc::new(ProcessorExchange::new(1)),
            EngineGraphControllerConfig {
                format: FORMAT,
                meters: default_peak_meter_bank(),
                initial_bindings: vec![],
                endpoint_backend: Arc::new(ContractOnlyVirtualEndpointBackend::default()),
                required_endpoint_node_id: None,
                processor_catalog: ProcessorCatalog::default(),
                graph_resources: Arc::new(NoopGraphResourceBackend),
                runtime_status: Arc::new(FixedRuntimeStatus::new(DriverState::Ready)),
            },
            GraphDocument::new(1, vec![], vec![]),
        );
        assert!(controller.graph().nodes.is_empty());
        assert_eq!(controller.graph_status().active_revision, 1);
    }

    const FORMAT: RenderFormat = RenderFormat {
        sample_rate: 48_000,
        block_frames: 128,
    };

    fn fail_render(_execution: &ExecutionPlan) -> Result<RenderPlan, RenderPlanError> {
        Err(RenderPlanError::BufferSizeOverflow)
    }

    fn controller(exchange: Arc<ProcessorExchange>) -> EngineGraphController {
        EngineGraphController::new(exchange, DEFAULT_ENDPOINT_NODE_ID, FORMAT)
    }

    fn processor(document: &GraphDocument) -> BlockProcessor {
        let engine = controller(Arc::new(ProcessorExchange::new(document.revision)));
        *engine
            .prepare_and_validate(
                document,
                CompileContext {
                    format: FORMAT,
                    limits: CompileLimits::default(),
                },
            )
            .expect("test graph should compose and prepare")
            .processor
    }

    #[test]
    fn builds_direct_and_gain_use_case_documents() {
        let direct = default_duplex_document(1, None);
        assert_eq!(direct.nodes.len(), 1);
        assert_eq!(direct.edges.len(), 2);
        let gain = default_duplex_document(2, Some(-6.0));
        assert_eq!(gain.nodes.len(), 2);
        assert_eq!(gain.edges.len(), 4);
    }

    #[test]
    fn exposes_runtime_status_and_submits_a_valid_gain_graph() {
        let exchange = Arc::new(ProcessorExchange::new(1));
        let controller = controller(Arc::clone(&exchange));
        assert_eq!(controller.status().driver_state, DriverState::Ready);
        assert_eq!(controller.status().sample_rate, Some(48_000));
        assert_eq!(controller.status().buffer_frames, Some(128));
        assert_eq!(controller.graph_status().active_revision, 1);

        let response = controller
            .apply_graph(default_duplex_document(2, Some(-6.020_600_3)))
            .expect("gain graph should submit");
        assert_eq!(response.revision, 2);
        assert_eq!(response.state, GraphApplyState::Pending);
        assert_eq!(controller.graph_status().submitted_revision, 2);
        assert_eq!(controller.graph().revision, 2);

        let mut current = Box::new(processor(&default_duplex_document(1, None)));
        assert!(exchange.activate_pending(&mut current));
        assert_eq!(current.plan().revision(), 2);
        assert_eq!(controller.graph_status().active_revision, 2);
    }

    #[test]
    fn rejects_stale_invalid_and_unsupported_graphs_without_replacing_audio() {
        let exchange = Arc::new(ProcessorExchange::new(3));
        let controller = controller(exchange);
        let stale = controller
            .apply_graph(default_duplex_document(3, None))
            .expect_err("same revision should be stale");
        assert_eq!(stale.code, ErrorCode::GraphRevisionStale);
        assert_eq!(stale.parameters["proposedRevision"], 3);

        let invalid = controller
            .apply_graph(GraphDocument::new(
                4,
                vec![NodeSpec {
                    id: NodeId::from("invalid-gain"),
                    label: None,
                    config: NodeConfig::Gain(GainConfig {
                        channels: 0,
                        gain_db: 0.0,
                    }),
                }],
                vec![],
            ))
            .expect_err("invalid graph should fail");
        assert_eq!(invalid.code, ErrorCode::GraphInvalid);
        assert!(invalid.parameters["issues"].is_array());

        let unsupported = controller
            .apply_graph(GraphDocument::new(5, vec![], vec![]))
            .expect_err("graph without the HAL endpoint should fail");
        assert_eq!(unsupported.code, ErrorCode::GraphBindingUnsupported);
        assert_eq!(
            unsupported.parameters["requiredEndpointNodeId"],
            DEFAULT_ENDPOINT_NODE_ID
        );
        assert_eq!(controller.graph().revision, 3);

        let mismatched = EngineGraphController::new(
            Arc::new(ProcessorExchange::new(1)),
            "another-endpoint",
            FORMAT,
        )
        .apply_graph(default_duplex_document(2, None))
        .expect_err("a different endpoint binding should fail");
        assert_eq!(mismatched.code, ErrorCode::GraphBindingUnsupported);
        assert_eq!(
            mismatched.parameters["requiredEndpointNodeId"],
            "another-endpoint"
        );
    }

    #[test]
    fn reports_render_preparation_and_exchange_failures() {
        let exchange = Arc::new(ProcessorExchange::new(1));
        let mut failing = controller(Arc::clone(&exchange));
        let catalog = ProcessorCatalog::default();
        failing.factory = Arc::new(BuiltInProcessorFactory {
            recorders: mixrelay_audio_recorder::RecorderRegistry::default(),
            catalog: catalog.clone(),
            hosted: ExternalProcessorFactory::new(catalog),
            meters: Arc::new(PeakMeterBank::new(64, 64)),
            render_prepare: fail_render,
        });
        let render = failing
            .apply_graph(default_duplex_document(2, None))
            .expect_err("render preparation should fail");
        assert_eq!(render.code, ErrorCode::GraphInvalid);
        assert!(render.parameters["renderError"].is_string());

        exchange
            .submit(Box::new(processor(&default_duplex_document(2, None))))
            .expect("test pending processor should queue");
        let before_prepare = failing
            .prepare_and_validate(&default_duplex_document(3, None), *failing.context.read())
            .err()
            .expect("pending activation must block preparation before its failing factory");
        assert_eq!(before_prepare.code, ErrorCode::GraphExchangeBusy);
        let busy = controller(exchange)
            .apply_graph(default_duplex_document(3, None))
            .expect_err("second pending graph should be busy");
        assert_eq!(busy.code, ErrorCode::GraphExchangeBusy);
        assert_eq!(busy.parameters["proposedRevision"], 3);
    }
}
