//! Assembles graph, native resource, persistence and control owners for the engine.

mod control_api;
mod default_graph;
mod endpoint_bindings;
mod graph_publication;
mod meter_readback;
mod native_resources;
mod processor_control;
mod processor_preparation;
mod session_change;

pub use default_graph::{default_duplex_document, default_session_document};
use endpoint_bindings::{validate_endpoint_bindings, virtual_endpoint_specs};
pub use meter_readback::default_peak_meter_bank;
use native_resources::ContractOnlyVirtualEndpointBackend;
pub use native_resources::{
    FixedRuntimeStatus, GraphResourceBackend, NoopGraphResourceBackend, PlaybackBindings,
    PreparedGraphFormat,
};
use processor_control::{
    persist_processor_parameter, processor_editor_error, processor_parameter_error,
};
#[cfg(test)]
use processor_preparation::PrepareProcessorError;
use processor_preparation::{BuiltInProcessorFactory, ProcessorFactory, prepare_error};
use session_change::{SessionChangeOrigin, SessionPauseOwner, session_error};

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use parking_lot::{Mutex, RwLock};
pub mod crash_guard;
mod engine_power;
mod execution_choice;
mod feature_policy;
mod meter_snapshot;
mod node_control;
mod node_control_snapshot;
mod recorder_control;
mod recorder_sessions;
mod runtime_node_control;
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
    ProcessorRetryRequest, ProcessorScanRequest, ProtocolError, SessionReconfigureRequest,
};
use mixrelay_control_server::ControlBackend;
use mixrelay_processor_host::{
    CatalogNodeResolver, ExternalProcessorFactory, ProcessorCatalog, ProcessorControlRegistry,
};

mod runtime_backend;
pub use runtime_backend::{
    ProcessingControlBackend, RuntimeBackend, RuntimeStatusBackend, SessionFormatBackend,
};

pub const DEFAULT_ENDPOINT_NODE_ID: &str = "default-virtual-io";

#[cfg(target_os = "macos")]
use mixrelay_macos_transport as _;

#[cfg(target_os = "macos")]
#[path = "macos/endpoint_service/mod.rs"]
pub mod endpoint_service;

mod control_maintenance;
pub mod control_service;
mod control_telemetry;
#[cfg(test)]
mod controller_startup_tests;
#[cfg(test)]
mod controller_test_support;
pub mod device_worker;
mod persistence;
pub use persistence::config_directory;
mod processor_latency;
mod processor_recovery;
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

#[cfg(test)]
mod pcm_fifo_bench;
pub mod render_demand;
pub mod resource_reclaimer;
pub mod resource_requirements;
pub mod session;
#[cfg(test)]
mod session_change_tests;
mod session_preferences;
#[cfg(test)]
mod virtual_source_gain_tests;
mod worker_join;

pub struct EngineGraphController {
    paid_access: Arc<dyn Fn() -> bool + Send + Sync>,
    power: execution_choice::ExecutionChoice,
    /// Random identity of this engine run; telemetry from a replaced engine never mixes in.
    incarnation: String,
    node_control_warning: Mutex<Option<String>>,
    recorders: recorder_sessions::RecorderSessions,
    exchange: Arc<ProcessorExchange>,
    factory: Arc<dyn ProcessorFactory>,
    graph: RwLock<GraphDocument>,
    context: RwLock<CompileContext>,
    mutation: Mutex<()>,
    catalog: graph_catalog::CatalogStorage,
    catalog_restore: graph_catalog::CatalogRecall,
    required_endpoint_node_id: Option<NodeId>,
    endpoint_backend: Arc<dyn VirtualEndpointBackend>,
    meter_bindings: RwLock<BTreeMap<u64, Vec<MeterBinding>>>,
    execution_views: RwLock<BTreeMap<u64, execution_control::ExecutionView>>,
    control_snapshot: RwLock<Option<node_control_snapshot::ControlReadView>>,
    meters: Arc<PeakMeterBank>,
    meter_publisher: Mutex<meter_snapshot::MeterPublisher>,
    telemetry_status: Mutex<Option<control_telemetry::CachedStatus>>,
    processor_catalog: ProcessorCatalog,
    processor_controls: ProcessorControlRegistry,
    graph_resources: Arc<dyn GraphResourceBackend>,
    runtime_status: Arc<dyn RuntimeBackend>,
}

pub struct EngineGraphControllerConfig {
    pub format: RenderFormat,
    pub meters: Arc<PeakMeterBank>,
    pub initial_bindings: Vec<MeterBinding>,
    pub endpoint_backend: Arc<dyn VirtualEndpointBackend>,
    pub required_endpoint_node_id: Option<NodeId>,
    pub processor_catalog: ProcessorCatalog,
    pub graph_resources: Arc<dyn GraphResourceBackend>,
    pub runtime_status: Arc<dyn RuntimeBackend>,
    /// Whether paid access is active, read at each admission. Production passes
    /// `mixrelay_licensing::has_local_paid_access`; tests pass a fixed answer so they never
    /// reach the user's keychain.
    pub paid_access: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl EngineGraphController {
    /// Constructs a controller bound to `endpoint_node_id` with contract-only endpoint,
    /// resource and Driver backends that route no OS audio. `paid_access` has no default
    /// so every caller decides admission explicitly.
    #[must_use]
    pub fn new(
        exchange: Arc<ProcessorExchange>,
        endpoint_node_id: impl Into<NodeId>,
        format: RenderFormat,
        paid_access: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        Self::new_with_configuration(
            exchange,
            EngineGraphControllerConfig {
                format,
                meters: default_peak_meter_bank(),
                initial_bindings: Vec::new(),
                endpoint_backend: Arc::new(ContractOnlyVirtualEndpointBackend::default()),
                required_endpoint_node_id: Some(endpoint_node_id.into()),
                processor_catalog: ProcessorCatalog::default(),
                graph_resources: Arc::new(NoopGraphResourceBackend),
                runtime_status: Arc::new(FixedRuntimeStatus::new(DriverState::Ready)),
                paid_access,
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
            paid_access,
        } = config;
        let active_revision = exchange.revisions().active;
        let mut meter_bindings = BTreeMap::new();
        meter_bindings.insert(active_revision, initial_bindings);
        let processor_controls = ProcessorControlRegistry::default();
        let recorders = mixrelay_audio_recorder::RecorderRegistry::default();
        let controller = Self {
            paid_access,
            node_control_warning: Mutex::new(None),
            power: execution_choice::ExecutionChoice::new(),
            incarnation: new_incarnation(),
            recorders: recorder_sessions::RecorderSessions::new(recorders.clone()),
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
            catalog: graph_catalog::CatalogStorage::default(),
            catalog_restore: graph_catalog::CatalogRecall::default(),
            required_endpoint_node_id,
            endpoint_backend,
            meter_bindings: RwLock::new(meter_bindings),
            execution_views: RwLock::new(BTreeMap::new()),
            control_snapshot: RwLock::new(None),
            telemetry_status: Mutex::new(None),
            meter_publisher: Mutex::new(meter_snapshot::MeterPublisher::new(Arc::clone(&meters))),
            meters,
            processor_catalog,
            processor_controls,
            graph_resources,
            runtime_status,
        };
        controller.refresh_control_snapshot();
        controller
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
        let prepared = self.prepare_execution(document, catalog.as_ref(), context)?;
        self.execution_views
            .write()
            .insert(document.revision, prepared.view.clone());
        Ok(prepared)
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
            if !node.control.enabled {
                continue;
            }
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

/// 128 random bits from two independently keyed hashes of this process and its start.
pub(crate) fn new_incarnation() -> String {
    use std::hash::{BuildHasher as _, RandomState};
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let seed = (std::process::id(), started);
    format!(
        "{:016x}{:016x}",
        RandomState::new().hash_one(seed),
        RandomState::new().hash_one(seed)
    )
}

#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
