use serde::{Deserialize, Serialize};
mod telemetry;
pub use telemetry::{ControlStateSnapshot, ControlTelemetry, TelemetryDecoder, TelemetryEncoder};
mod editor_surface;
mod node_control;
mod workspace;
pub use editor_surface::{EditorPointerKind, ProcessorEditorFrame, ProcessorEditorInput};
pub use mixrelay_audio_recorder::{RecorderRequest, RecorderStatus};
pub use node_control::{
    EngineControlSnapshot, GraphControlSnapshot, GraphNodeControls, NodeControlCapabilities,
    NodeControlDescriptor, NodeControlPreview, NodeControlRequest, NodeControlResponse,
    NodeControlSnapshot, ObservedRecording, RecorderControlRequest,
};
pub use workspace::{WorkspaceAction, WorkspacePreview, WorkspaceRequest};

/// Engine-authored capture ownership, independent of the graph visible in an editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationCaptureManifest {
    pub protocol_version: u16,
    pub revision: u64,
    pub sources: Vec<ApplicationCaptureSource>,
}

/// One canonical capture owner and the exact request its native adapter must fulfill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplicationCaptureSource {
    pub node_id: String,
    pub request: mixrelay_audio_io::ApplicationCaptureRequest,
}
pub use mixrelay_audio_io::session::SessionReconfigureRequest;

/// Changes one identified virtual device, not the processing clock of unrelated routes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VirtualEndpointFormatRequest {
    pub expected_revision: u64,
    pub endpoint_id: mixrelay_audio_io::VirtualEndpointId,
    pub sample_rate: u32,
}

pub const PROTOCOL_VERSION: u16 = 1;

/// Publishes graph topology while retaining surviving providers' native state.
/// Only explicit, bounded control edits overlay that state during publication.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphApplyRequest {
    #[serde(flatten)]
    pub document: mixrelay_audio_graph::GraphDocument,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub processor_parameter_changes: Vec<GraphProcessorParameterChange>,
    /// The graph this document was authored for. When present, the engine rejects the
    /// request as stale unless that graph is still selected, so a selection change by
    /// another client cannot redirect the document onto a different graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_graph_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphProcessorParameterChange {
    pub node_id: String,
    pub type_id: String,
    pub parameter_id: u32,
    pub value: f64,
}

impl From<mixrelay_audio_graph::GraphDocument> for GraphApplyRequest {
    fn from(document: mixrelay_audio_graph::GraphDocument) -> Self {
        Self {
            document,
            processor_parameter_changes: Vec::new(),
            expected_graph_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphCatalogEntry {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphCatalogResponse {
    pub protocol_version: u16,
    pub catalog_revision: u64,
    pub active_id: String,
    pub graphs: Vec<GraphCatalogEntry>,
    pub document: mixrelay_audio_graph::GraphDocument,
    #[serde(default)]
    pub layout_revision: u64,
    #[serde(default)]
    pub layout: GraphLayout,
    /// Audio publication succeeded but disk replacement failed; never report a rollback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence_error: Option<String>,
}

/// Presentation coordinates use UI identities, not DSP node identities. A duplex endpoint
/// may have two independently positioned UI cards backed by one runtime graph node.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphLayout {
    /// Display-only names for graph UI instances. Never used as application/device identities.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub node_labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub positions: std::collections::BTreeMap<String, GraphNodePosition>,
    /// User-resized cards keyed by UI identity; absent entries use their natural size.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub sizes: std::collections::BTreeMap<String, GraphNodeSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<GraphViewport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNodePosition {
    pub x: f64,
    pub y: f64,
}

/// Explicit UI card dimensions in graph coordinates, independent of the audio document.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNodeSize {
    /// Card width in graph pixels.
    pub width: f64,
    /// Card height in graph pixels.
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphViewport {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphLayoutRequest {
    pub expected_layout_revision: u64,
    pub layout: GraphLayout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphLayoutResponse {
    pub graph_id: String,
    pub layout_revision: u64,
    pub layout: GraphLayout,
}

/// Catalog edits fenced by both the execution and catalog revisions in the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GraphCatalogAction {
    Create {
        name: String,
    },
    Select {
        id: String,
    },
    Rename {
        id: String,
        name: String,
    },
    /// Removes one authored graph and its execution owners. The final removal creates
    /// a new empty graph with a fresh identity, never reusing the removed target.
    Delete {
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphCatalogRequest {
    pub expected_revision: u64,
    pub expected_catalog_revision: u64,
    #[serde(flatten)]
    pub action: GraphCatalogAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverState {
    NotInstalled,
    Installing,
    RestartRequired,
    InstalledNotLoaded,
    Blocked,
    /// Installed transport cannot currently transfer audio; distinct from OS policy blocking.
    Unavailable,
    InvalidPermissions,
    VersionMismatch,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    /// Execution switch state; absent when the backend cannot control execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_power: Option<EnginePowerStatus>,
    /// Fresh per-processor transport samples; identities remain graph-local.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub processor_timings: Vec<ProcessorTiming>,
    /// Prepared transport delay and cached native latency, independent of wall-clock timing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub processor_latencies: Vec<ProcessorLatency>,
    /// Failed hosted processors that still belong to the active execution snapshot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed_processors: Vec<FailedProcessor>,
    /// Finalization error from a removed recorder, retained independently of its node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorder_error: Option<String>,
    pub protocol_version: u16,
    pub driver_state: DriverState,
    pub sample_rate: Option<u32>,
    pub buffer_frames: Option<u32>,
    pub underruns: u64,
    pub overruns: u64,
    /// Lifetime non-finite PCM replacements by Core boundary; no audio or node identities.
    #[serde(default)]
    pub sample_faults: mixrelay_audio_io::process::SampleFaults,
    /// Absent in older engines. UI must not manufacture capabilities when unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_session: Option<mixrelay_audio_io::session::AudioSessionStatus>,
}

/// Requested execution state and whether native owners are still transitioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnginePowerStatus {
    pub enabled: bool,
    pub transitioning: bool,
    /// The service could not yet save this state for its next start; it keeps retrying.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsaved: bool,
    /// Why the engine paused itself; absent for a pause the user chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<EnginePauseReason>,
}

/// A pause the engine chose rather than the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EnginePauseReason {
    /// It ended unexpectedly several times in a row; it stays paused until Resume.
    RepeatedFailures,
}

/// Idempotent execution command; never changes authored graphs or service installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnginePowerRequest {
    pub enabled: bool,
    /// Optional compare-and-set revision supplied by native capture preflight.
    pub expected_revision: Option<u64>,
}

/// One sampled block's engine-to-worker round trip, not hardware or plug-in reported latency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorTiming {
    pub graph_id: String,
    pub node_id: String,
    pub round_trip_ns: u64,
}

/// A live prepared processor's latency snapshot in its processing-rate sample frames.
///
/// This is observation only: it does not imply that graph paths are delay-compensated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorLatency {
    /// Authored graph identity, independent of the graph currently viewed in the editor.
    pub graph_id: String,
    /// Authored node identity within `graph_id`.
    pub node_id: String,
    /// Fixed engine-to-worker audio delay; excludes scheduling time and native latency.
    pub transport_frames: u32,
    /// Cached native latency; `None` is unknown, while `Some(0)` is a reported zero.
    pub intrinsic_frames: Option<u32>,
}

/// A failed active provider that can be explicitly retried against this exact worker instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FailedProcessor {
    pub graph_id: String,
    pub node_id: String,
    /// Decimal `u64` token serialized as text to preserve exact identity in JavaScript clients.
    pub instance_id: String,
}

/// Explicit retry of one failed active provider, guarded by graph, catalog and worker identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessorRetryRequest {
    pub graph_id: String,
    pub node_id: String,
    pub expected_revision: u64,
    pub expected_catalog_revision: u64,
    pub expected_instance_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphApplyState {
    Pending,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRuntimeStatus {
    pub protocol_version: u16,
    pub active_revision: u64,
    pub submitted_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMeterSnapshot {
    pub node_id: String,
    pub port_id: String,
    pub peaks: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamics: Option<DynamicsMeterSnapshot>,
}

/// Block peak input and actual maximum detector attenuation since the previous meter poll.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicsMeterSnapshot {
    pub input_peaks: Vec<f32>,
    pub gain_reduction_db: f32,
    pub threshold_db: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterSnapshot {
    pub protocol_version: u16,
    pub revision: u64,
    pub nodes: Vec<NodeMeterSnapshot>,
}

/// One pushed meter window. The engine incarnation identifies the engine process that
/// measured it: windows from a replaced engine never mix with its successor's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterTelemetry {
    pub engine_incarnation: String,
    /// Increases with every window this subscription delivers; gaps are allowed.
    pub sequence: u64,
    pub meters: MeterSnapshot,
}

impl MeterTelemetry {
    /// Folds a newer window into this undelivered one, so a slow subscriber keeps the
    /// latest state and the interval maximum instead of a queue. Peaks and gain reduction
    /// take the maximum per binding; everything else follows the newer window. A window
    /// from another engine or plan revision replaces this one, since its bindings differ.
    pub fn absorb(&mut self, newer: Self) {
        if newer.engine_incarnation != self.engine_incarnation
            || newer.meters.protocol_version != self.meters.protocol_version
            || newer.meters.revision != self.meters.revision
        {
            *self = newer;
            return;
        }
        let mut older = std::mem::take(&mut self.meters.nodes);
        self.sequence = newer.sequence;
        self.meters = newer.meters;
        for node in &mut self.meters.nodes {
            let Some(index) = older
                .iter()
                .position(|old| old.node_id == node.node_id && old.port_id == node.port_id)
            else {
                continue;
            };
            let old = older.swap_remove(index);
            max_peaks(&mut node.peaks, &old.peaks);
            if let (Some(dynamics), Some(old)) = (&mut node.dynamics, old.dynamics) {
                max_peaks(&mut dynamics.input_peaks, &old.input_peaks);
                dynamics.gain_reduction_db = dynamics.gain_reduction_db.max(old.gain_reduction_db);
            }
        }
    }
}

/// Element-wise maximum when both windows describe the same channels.
fn max_peaks(peaks: &mut [f32], older: &[f32]) {
    if peaks.len() == older.len() {
        for (peak, old) in peaks.iter_mut().zip(older) {
            *peak = peak.max(*old);
        }
    }
}

#[cfg(test)]
mod meter_telemetry_tests {
    use super::{DynamicsMeterSnapshot, MeterSnapshot, MeterTelemetry, NodeMeterSnapshot};

    fn node(id: &str, peaks: &[f32], reduction: Option<f32>) -> NodeMeterSnapshot {
        NodeMeterSnapshot {
            node_id: id.to_owned(),
            port_id: "out".to_owned(),
            peaks: peaks.to_vec(),
            dynamics: reduction.map(|gain_reduction_db| DynamicsMeterSnapshot {
                input_peaks: peaks.to_vec(),
                gain_reduction_db,
                threshold_db: -gain_reduction_db,
            }),
        }
    }

    fn window(
        engine: &str,
        revision: u64,
        sequence: u64,
        nodes: Vec<NodeMeterSnapshot>,
    ) -> MeterTelemetry {
        MeterTelemetry {
            engine_incarnation: engine.to_owned(),
            sequence,
            meters: MeterSnapshot {
                protocol_version: 1,
                revision,
                nodes,
            },
        }
    }

    #[test]
    fn a_slow_subscriber_keeps_the_interval_maximum_of_the_latest_bindings() {
        let mut pending = window(
            "a",
            3,
            1,
            vec![
                node("loud", &[0.9, 0.1], Some(6.0)),
                node("gone", &[1.0], None),
                node("resized", &[0.8], None),
                node("plain", &[0.7], Some(1.0)),
            ],
        );
        pending.absorb(window(
            "a",
            3,
            2,
            vec![
                node("loud", &[0.2, 0.5], Some(2.0)),
                node("new", &[0.3], None),
                node("resized", &[0.1, 0.1], None),
                node("plain", &[0.2], None),
            ],
        ));
        assert_eq!(
            pending,
            window(
                "a",
                3,
                2,
                vec![
                    NodeMeterSnapshot {
                        dynamics: Some(DynamicsMeterSnapshot {
                            input_peaks: vec![0.9, 0.5],
                            gain_reduction_db: 6.0,
                            threshold_db: -2.0,
                        }),
                        ..node("loud", &[0.9, 0.5], None)
                    },
                    node("new", &[0.3], None),
                    node("resized", &[0.1, 0.1], None),
                    node("plain", &[0.7], None),
                ],
            )
        );
    }

    #[test]
    fn another_engine_or_plan_replaces_the_pending_window() {
        let newer = window("b", 3, 1, vec![node("x", &[0.1], None)]);
        let mut pending = window("a", 3, 9, vec![node("x", &[1.0], None)]);
        pending.absorb(newer.clone());
        assert_eq!(pending, newer);
        let newer = window("b", 4, 2, vec![node("x", &[0.1], None)]);
        pending.absorb(newer.clone());
        assert_eq!(pending, newer);
        let mut newer = window("b", 4, 3, vec![node("x", &[0.1], None)]);
        newer.meters.protocol_version = 2;
        pending.absorb(newer.clone());
        assert_eq!(pending, newer);
    }
}

#[cfg(test)]
mod dynamics_meter_contract_tests {
    use super::{DynamicsMeterSnapshot, NodeMeterSnapshot};

    #[test]
    fn legacy_meter_json_remains_valid_and_actual_dynamics_roundtrips_without_peak_inference() {
        let mut meter: NodeMeterSnapshot = serde_json::from_value(serde_json::json!({
            "nodeId":"compressor", "portId":"audio", "peaks":[1.5,0.75],
        }))
        .unwrap();
        assert!(meter.dynamics.is_none());
        assert!(
            serde_json::to_value(&meter)
                .unwrap()
                .get("dynamics")
                .is_none()
        );
        meter.dynamics = Some(DynamicsMeterSnapshot {
            input_peaks: vec![1.0, 0.5],
            gain_reduction_db: 13.5,
            threshold_db: -18.0,
        });
        let encoded = serde_json::to_value(&meter).unwrap();
        assert_eq!(encoded["dynamics"]["gainReductionDb"], 13.5);
        assert_eq!(encoded["dynamics"]["thresholdDb"], -18.0);
        assert_eq!(
            serde_json::from_value::<NodeMeterSnapshot>(encoded).unwrap(),
            meter
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalProcessorFormat {
    Au,
    Auv3,
    Clap,
    Lv2,
    Vst3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorParameterDescriptor {
    pub id: u32,
    pub name: String,
    pub default_value: f64,
    pub step_count: i32,
    pub can_automate: bool,
    pub is_read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalProcessorDescriptor {
    pub type_id: String,
    pub display_name: String,
    pub vendor: String,
    pub version: String,
    pub format: ExternalProcessorFormat,
    /// Exact native bus configurations validated by the disposable probe.
    pub layouts: Vec<mixrelay_audio_graph::ProviderPorts>,
    /// Preferred member of `layouts` for insertion without a manual selection.
    pub default_layout_id: String,
    pub has_editor: bool,
    pub parameters: Vec<ProcessorParameterDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalProcessorCatalog {
    pub protocol_version: u16,
    pub processors: Vec<ExternalProcessorDescriptor>,
    /// A partial scan retains previously usable entries and reports which discovery stages failed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<ExternalProcessorDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalProcessorDiagnostic {
    pub format: ExternalProcessorFormat,
    pub stage: ProcessorDiscoveryStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorDiscoveryStage {
    Scan,
    Probe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorScanRequest {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessorParameterUpdate {
    /// Revision observed when the caller chose this target.
    pub expected_revision: u64,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorParameterUpdateResponse {
    pub protocol_version: u16,
    pub node_id: String,
    pub parameter_id: u32,
    pub value: f64,
    /// The native parameter was accepted even when durable graph storage failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorEditorState {
    Unavailable,
    Closed,
    Opening,
    Open,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorEditorFailure {
    NoEditor,
    DisplayUnavailable,
    HostWindowFailed,
    PluginRejected,
    WorkerUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessorEditorRequest {
    /// Revision observed when the caller chose this target.
    pub expected_revision: u64,
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessorEditorStatus {
    pub protocol_version: u16,
    pub node_id: String,
    pub state: ProcessorEditorState,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub resizable: bool,
    pub failure: Option<ProcessorEditorFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphApplyResponse {
    pub protocol_version: u16,
    pub revision: u64,
    pub state: GraphApplyState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence_error: Option<String>,
}

impl Default for AppStatus {
    fn default() -> Self {
        Self {
            engine_power: None,
            processor_timings: Vec::new(),
            processor_latencies: Vec::new(),
            failed_processors: Vec::new(),
            recorder_error: None,
            protocol_version: PROTOCOL_VERSION,
            driver_state: DriverState::NotInstalled,
            sample_rate: None,
            buffer_frames: None,
            underruns: 0,
            overruns: 0,
            sample_faults: mixrelay_audio_io::process::SampleFaults::default(),
            audio_session: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    DriverNotInstalled,
    DriverNotLoaded,
    DriverPermissionInvalid,
    DriverVersionMismatch,
    AudioDeviceDisconnected,
    BufferUnderrun,
    BufferOverrun,
    AudioSourceUnavailable,
    PairingRequired,
    Unauthorized,
    ControlUnavailable,
    /// The expected engine process has been replaced; the operation was not admitted.
    EngineReplaced,
    /// The client operation request header is malformed.
    InvalidOperationId,
    GraphInvalid,
    GraphRevisionStale,
    GraphExchangeBusy,
    GraphBindingUnsupported,
    /// The addressed authored node no longer exists.
    NodeNotFound,
    /// Processing is disabled; the command cannot act on runtime resources.
    NodeDisabled,
    ProcessorUnavailable,
    RecorderFailed,
    /// A free-edition quota or paid-only operation was exceeded.
    FeatureLimit,
    AudioSessionUnsupported,
    AudioSessionProcessorStateUnavailable,
    AudioSessionReconfigureFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub parameters: serde_json::Value,
}

#[cfg(test)]
mod tests {
    #[test]
    fn latency_status_distinguishes_unknown_from_zero_and_transport_timing() {
        let mut status = super::AppStatus::default();
        assert!(
            serde_json::to_value(&status)
                .unwrap()
                .get("processorLatencies")
                .is_none()
        );
        for (graph, latency) in [
            ("graph-a", None),
            ("graph-b", Some(0)),
            ("graph-c", Some(u32::MAX)),
        ] {
            status.processor_latencies.push(super::ProcessorLatency {
                graph_id: graph.to_owned(),
                node_id: "effect".to_owned(),
                transport_frames: 128,
                intrinsic_frames: latency,
            });
        }
        let value = serde_json::to_value(&status).unwrap();
        assert!(value.get("processorTimings").is_none());
        let entries = &value["processorLatencies"];
        assert_eq!(entries[0]["graphId"], "graph-a");
        assert_eq!(entries[1]["graphId"], "graph-b");
        assert_eq!(entries[0]["nodeId"], "effect");
        assert_eq!(entries[0]["transportFrames"], 128);
        assert!(entries[0]["intrinsicFrames"].is_null());
        assert_eq!(entries[1]["intrinsicFrames"], 0);
        assert_eq!(entries[2]["intrinsicFrames"], u32::MAX);
        let decoded: super::AppStatus = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.processor_latencies, status.processor_latencies);
    }

    #[test]
    fn processor_timing_status_preserves_graph_identity_and_omits_unmeasured_values() {
        let mut status = super::AppStatus::default();
        let empty = serde_json::to_value(&status).unwrap();
        assert!(empty.get("processorTimings").is_none());
        status.processor_timings.push(super::ProcessorTiming {
            graph_id: "graph-a".to_owned(),
            node_id: "effect-1".to_owned(),
            round_trip_ns: 1_333_333,
        });
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["processorTimings"][0]["graphId"], "graph-a");
        assert_eq!(value["processorTimings"][0]["roundTripNs"], 1_333_333);
        let decoded: super::AppStatus = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.processor_timings, status.processor_timings);
    }

    #[test]
    fn failed_processor_status_and_retry_identity_are_exact_and_optional() {
        let mut status = super::AppStatus::default();
        assert!(
            serde_json::to_value(&status)
                .unwrap()
                .get("failedProcessors")
                .is_none()
        );
        status.failed_processors.push(super::FailedProcessor {
            graph_id: "graph-a".into(),
            node_id: "effect".into(),
            instance_id: "18446744073709551615".into(),
        });
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(
            value["failedProcessors"][0]["instanceId"],
            "18446744073709551615"
        );
        let decoded: super::AppStatus = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.failed_processors, status.failed_processors);

        let request = super::ProcessorRetryRequest {
            graph_id: "graph-a".into(),
            node_id: "effect".into(),
            expected_revision: 12,
            expected_catalog_revision: 7,
            expected_instance_id: "18446744073709551615".into(),
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["expectedInstanceId"], "18446744073709551615");
        assert_eq!(
            serde_json::from_value::<super::ProcessorRetryRequest>(value).unwrap(),
            request
        );
    }

    #[test]
    fn presentation_metadata_is_optional_for_pre_layout_catalog_responses() {
        let old = serde_json::json!({
            "protocolVersion":1,"catalogRevision":1,"activeId":"graph-1",
            "graphs":[{"id":"graph-1","name":"Before layouts"}],
            "document":{"schemaVersion":2,"revision":1,"nodes":[],"edges":[]}
        });
        let response: super::GraphCatalogResponse = serde_json::from_value(old).unwrap();
        assert_eq!(response.layout_revision, 0);
        assert_eq!(response.layout, super::GraphLayout::default());
        let layout = serde_json::json!({"expectedLayoutRevision":3,"layout":{
            "positions":{"duplex:from_os":{"x":100.0,"y":-20.0}},
            "sizes":{"duplex:from_os":{"width":320.0,"height":180.0}},
            "viewport":{"x":0.0,"y":0.0,"zoom":1.25}
        }});
        let request: super::GraphLayoutRequest = serde_json::from_value(layout.clone()).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), layout);
    }

    #[test]
    fn graph_ui_instance_labels_are_optional_and_round_trip_separately() {
        let legacy: super::GraphLayout =
            serde_json::from_value(serde_json::json!({"positions":{}})).unwrap();
        assert!(legacy.node_labels.is_empty());
        assert_eq!(
            serde_json::to_value(legacy).unwrap(),
            serde_json::json!({"positions":{}})
        );
        let value =
            serde_json::json!({"positions":{}, "nodeLabels":{"app-1":"영상용", "app-2":"회의용"}});
        let layout: super::GraphLayout = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(layout).unwrap(), value);
    }

    #[test]
    fn unavailable_driver_is_distinct_from_policy_block_and_round_trips() {
        let status = super::AppStatus {
            driver_state: super::DriverState::Unavailable,
            ..super::AppStatus::default()
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["driverState"], "unavailable");
        let decoded: super::AppStatus = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.driver_state, super::DriverState::Unavailable);
        assert_ne!(decoded.driver_state, super::DriverState::Blocked);
    }
    use super::*;

    #[test]
    fn sample_fault_status_preserves_legacy_status_and_serializes_boundary_counts() {
        let mut value = serde_json::to_value(AppStatus::default()).unwrap();
        value.as_object_mut().unwrap().remove("sampleFaults");
        let legacy: AppStatus = serde_json::from_value(value).unwrap();
        assert_eq!(
            legacy.sample_faults,
            mixrelay_audio_io::process::SampleFaults::default()
        );

        let status = AppStatus {
            sample_faults: mixrelay_audio_io::process::SampleFaults {
                source_samples: 2,
                input_samples: 3,
                output_samples: 4,
                blocks: 1,
            },
            ..Default::default()
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(
            value["sampleFaults"],
            serde_json::json!({"sourceSamples":2,"inputSamples":3,"outputSamples":4,"blocks":1})
        );
        assert_eq!(serde_json::from_value::<AppStatus>(value).unwrap(), status);
    }

    #[test]
    fn status_contract_uses_stable_machine_values() {
        let json = serde_json::to_value(AppStatus::default()).expect("status should serialize");

        assert_eq!(json["protocolVersion"], 1);
        assert_eq!(json["driverState"], "not_installed");
        assert!(json["sampleRate"].is_null());
        assert!(json["bufferFrames"].is_null());

        let runtime = serde_json::to_value(GraphRuntimeStatus {
            protocol_version: PROTOCOL_VERSION,
            active_revision: 4,
            submitted_revision: 5,
        })
        .expect("runtime status should serialize");
        assert_eq!(runtime["activeRevision"], 4);
        assert_eq!(runtime["submittedRevision"], 5);

        let applied = serde_json::to_value(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 5,
            state: GraphApplyState::Pending,
            persistence_error: None,
        })
        .expect("apply response should serialize");
        assert_eq!(applied["state"], "pending");
        assert_eq!(
            serde_json::to_value(GraphApplyState::Active).expect("state should serialize"),
            "active"
        );

        let error = serde_json::to_value(ProtocolError {
            code: ErrorCode::GraphBindingUnsupported,
            parameters: serde_json::json!({ "nodeId": "virtual" }),
        })
        .expect("protocol error should serialize");
        assert_eq!(error["code"], "graph_binding_unsupported");
        assert_eq!(error["parameters"]["nodeId"], "virtual");

        let parameter = serde_json::to_value(ProcessorParameterUpdateResponse {
            protocol_version: PROTOCOL_VERSION,
            node_id: "vst-1".to_owned(),
            parameter_id: 42,
            value: 0.75,
            persistence_error: None,
        })
        .expect("parameter response should serialize");
        assert_eq!(
            serde_json::to_value(ExternalProcessorFormat::Au).expect("format should serialize"),
            "au"
        );
        assert_eq!(
            serde_json::to_value(ExternalProcessorFormat::Auv3).expect("format should serialize"),
            "auv3"
        );
        assert_eq!(
            serde_json::to_value(ExternalProcessorFormat::Clap).expect("format should serialize"),
            "clap"
        );
        assert_eq!(
            serde_json::to_value(ExternalProcessorFormat::Lv2).expect("format should serialize"),
            "lv2"
        );
        assert_eq!(parameter["nodeId"], "vst-1");
        assert_eq!(parameter["parameterId"], 42);
        assert_eq!(parameter["value"], 0.75);

        let editor = serde_json::to_value(ProcessorEditorStatus {
            protocol_version: PROTOCOL_VERSION,
            node_id: "processor-1".to_owned(),
            state: ProcessorEditorState::Failed,
            width: Some(640),
            height: Some(480),
            resizable: true,
            failure: Some(ProcessorEditorFailure::DisplayUnavailable),
        })
        .expect("editor status should serialize");
        assert_eq!(editor["nodeId"], "processor-1");
        assert_eq!(editor["state"], "failed");
        assert_eq!(editor["failure"], "display_unavailable");
        assert_eq!(editor["width"], 640);
    }

    #[test]
    fn parameter_storage_warning_is_optional_and_preserves_the_accepted_value() {
        let mut response: ProcessorParameterUpdateResponse = serde_json::from_value(serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION, "nodeId": "effect", "parameterId": 7, "value": 0.75,
        })).unwrap();
        assert!(
            response.persistence_error.is_none(),
            "old responses remain readable"
        );
        assert!(
            serde_json::to_value(&response)
                .unwrap()
                .get("persistenceError")
                .is_none()
        );
        response.persistence_error = Some("disk unavailable".into());
        let warned = serde_json::to_value(response).unwrap();
        assert_eq!(
            warned["value"], 0.75,
            "storage warning does not roll back accepted audio"
        );
        assert_eq!(warned["persistenceError"], "disk unavailable");
    }

    #[test]
    fn catalog_diagnostics_are_additive_and_preserve_format_stage_and_candidate_identity() {
        let mut catalog: ExternalProcessorCatalog = serde_json::from_value(serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION, "processors": []
        }))
        .unwrap();
        assert!(catalog.diagnostics.is_empty());
        assert!(
            serde_json::to_value(&catalog)
                .unwrap()
                .get("diagnostics")
                .is_none()
        );
        catalog.diagnostics.push(ExternalProcessorDiagnostic {
            format: ExternalProcessorFormat::Clap,
            stage: ProcessorDiscoveryStage::Probe,
            plugin_id: Some("clap:fixture".to_owned()),
            plugin_name: None,
            message: "child timed out".to_owned(),
        });
        let encoded = serde_json::to_value(&catalog).unwrap();
        assert_eq!(
            encoded["diagnostics"][0],
            serde_json::json!({
                "format":"clap", "stage":"probe", "pluginId":"clap:fixture", "message":"child timed out"
            })
        );
        assert_eq!(
            serde_json::from_value::<ExternalProcessorCatalog>(encoded).unwrap(),
            catalog
        );
    }

    #[test]
    fn graph_apply_explicit_control_intent_is_additive_to_legacy_documents() {
        let document = mixrelay_audio_graph::GraphDocument::new(2, vec![], vec![]);
        let legacy = serde_json::to_value(&document).unwrap();
        let mut request: GraphApplyRequest = serde_json::from_value(legacy.clone()).unwrap();
        assert!(request.processor_parameter_changes.is_empty());
        assert_eq!(serde_json::to_value(&request).unwrap(), legacy);
        request
            .processor_parameter_changes
            .push(GraphProcessorParameterChange {
                node_id: "effect".into(),
                type_id: "vst3:fixture".into(),
                parameter_id: 42,
                value: 0.25,
            });
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(
            encoded["processorParameterChanges"][0],
            serde_json::json!({
                "nodeId":"effect", "typeId":"vst3:fixture", "parameterId":42, "value":0.25
            })
        );
        assert_eq!(
            serde_json::from_value::<GraphApplyRequest>(encoded.clone()).unwrap(),
            request
        );
        assert_eq!(
            serde_json::from_value::<mixrelay_audio_graph::GraphDocument>(encoded.clone()).unwrap(),
            document,
            "desktop preflight can inspect the document while forwarding the complete intent unchanged"
        );
        let mut targeted = encoded.clone();
        targeted["expectedGraphId"] = "graph-1".into();
        assert_eq!(
            serde_json::from_value::<GraphApplyRequest>(targeted)
                .unwrap()
                .expected_graph_id
                .as_deref(),
            Some("graph-1")
        );
        let mut malformed = encoded;
        malformed["processorParameterChanges"][0]["stateBase64"] =
            "implicit preset replacement".into();
        assert!(serde_json::from_value::<GraphApplyRequest>(malformed).is_err());
    }
}

#[cfg(test)]
mod engine_fence_tests {
    #[test]
    fn invalid_operation_id_is_a_stable_wire_error() {
        let code = super::ErrorCode::InvalidOperationId;
        let value = serde_json::to_value(code).unwrap();
        assert_eq!(value, "invalid_operation_id");
        assert_eq!(
            serde_json::from_value::<super::ErrorCode>(value).unwrap(),
            code
        );
    }

    #[test]
    fn engine_replaced_is_a_stable_wire_error() {
        let error = super::ProtocolError {
            code: super::ErrorCode::EngineReplaced,
            parameters: serde_json::json!({"operationOutcome":"not_admitted"}),
        };
        let value = serde_json::to_value(&error).unwrap();
        assert_eq!(value["code"], "engine_replaced");
        assert_eq!(
            serde_json::from_value::<super::ProtocolError>(value).unwrap(),
            error
        );
    }
}
