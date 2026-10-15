export type DriverState =
  | 'not_installed'
  | 'installing'
  | 'restart_required'
  | 'installed_not_loaded'
  | 'blocked'
  | 'unavailable'
  | 'invalid_permissions'
  | 'version_mismatch'
  | 'ready';

export interface MixRelayStatus {
  /** `unsaved`: the service could not yet save this state for its next start and keeps retrying.
   * `pauseReason`: why the engine paused itself (absent for a pause the user chose);
   * `repeatedFailures` means it ended unexpectedly several times in a row. */
  enginePower?: {
    enabled: boolean;
    transitioning: boolean;
    unsaved?: boolean;
    pauseReason?: 'repeatedFailures';
  };
  failedProcessors?: FailedProcessorInstance[];
  processorTimings?: ProcessorTiming[];
  processorLatencies?: ProcessorLatency[];
  recorderError?: string | null;
  protocolVersion: 1;
  driverState: DriverState;
  sampleRate: number | null;
  bufferFrames: number | null;
  underruns: number;
  overruns: number;
  audioSession?: AudioSessionStatus;
}

/** Opaque native instance identity for a processor whose DSP instance has failed. */
export interface FailedProcessorInstance {
  graphId: string;
  instanceId: string;
  nodeId: string;
}

export interface ProcessorTiming {
  graphId: string;
  nodeId: string;
  roundTripNs: number;
}

/** Cached frame-domain latency, not a wall-clock measurement or PDC guarantee. */
export interface ProcessorLatency {
  graphId: string;
  nodeId: string;
  transportFrames: number;
  /** null means unknown; zero is a reported zero-latency value. */
  intrinsicFrames: number | null;
}

export interface AudioSessionStatus {
  endpointPreferencesUnavailable?: boolean;
  virtualEndpointSettings?: VirtualEndpointSettings;
  processingState?: 'idle' | 'running' | 'stopped';
  discontinuities?: number;
  format: { sampleRate: number; blockFrames: number };
  preferredFormat?: { sampleRate: number; blockFrames: number } | null;
  clockAuthority: 'engine_timer' | 'device' | 'audio_server' | 'host';
  observedDeviceSampleRate: number | null;
  observedDeviceBufferFrames?: number | null;
  issue:
    | 'processing_failed'
    | 'application_unavailable'
    | 'application_ambiguous'
    | 'device_unavailable'
    | 'sample_rate_mismatch'
    | 'block_size_mismatch'
    | 'clock_not_synchronized'
    | 'invalid_format'
    | 'reconfiguration_failed'
    | 'preferences_unavailable'
    | 'stored_format_unavailable'
    | null;
  transportLatencyFrames: number | null;
  capabilities: {
    backend: 'core_audio' | 'pipe_wire' | 'contract_only';
    supportedSampleRates: number[];
    supportedBlockFrames: number[];
    canReconfigureLive: boolean;
    hardwareInput: boolean;
    hardwareOutput: boolean;
    virtualEndpoint: boolean;
    applicationCapture: boolean;
    sampleRateConversion: boolean;
    driftCorrection: boolean;
  };
}

export interface VirtualEndpointSettings {
  canCreate: boolean;
  canRename: boolean;
  renderChannels: number[];
  captureChannels: number[];
  creationTemplate: Omit<VirtualEndpointSpec, 'id' | 'displayName'> | null;
  devices: VirtualEndpointDevice[];
}

export type NativeSampleFormat = 'float32' | 'signed16' | 'signed24' | 'signed32';

export interface NativeStreamFormat {
  sampleRate: number;
  channels: number;
  sampleFormat: NativeSampleFormat;
}

export interface NativeStreamFormats {
  current: NativeStreamFormat | null;
  supported: NativeStreamFormat[];
}

export interface NativeDeviceFormats {
  render: NativeStreamFormats;
  capture: NativeStreamFormats;
}

export interface VirtualEndpointDevice {
  nativeDeviceIds: string[];
  nativeFormats: NativeDeviceFormats;
  spec: VirtualEndpointSpec;
  observedSampleRate: number | null;
  supportedSampleRates: number[];
  canSetSampleRate: boolean;
}

export interface RuntimeStreamSpec {
  channelLayout: 'discrete' | 'mono' | 'stereo';
  channels: number;
}

export interface VirtualEndpointSpec {
  id: string;
  displayName: string;
  render?: RuntimeStreamSpec | null;
  capture?: RuntimeStreamSpec | null;
  supportedSampleRates: number[];
  defaultSampleRate: number;
}

export interface VirtualEndpointRateError {
  endpointId: string;
  sampleRate: number;
  message: string;
  kind: 'negotiation' | 'persistence';
}

export interface RuntimeVirtualEndpointConfig extends VirtualEndpointSpec {
  gainDb?: number;
  reference: { render: boolean; capture: boolean };
}

export type RuntimeApplicationSelector =
  | { bundle_id: string; include_descendants: boolean; match: 'bundle_id' }
  | { match: 'executable_path'; path: string }
  | { include_descendants: boolean; match: 'process_id'; process_id: number }
  | { bundle_ids: string[]; match: 'all_except' };

export interface CapturableApplication {
  bundleId: string;
  displayName: string;
  iconDataUrl: string | null;
  processId: number;
  selector: RuntimeApplicationSelector;
}

export interface AudioDeviceDescriptor {
  displayName: string;
  inputChannels: number;
  isDefaultInput: boolean;
  isDefaultOutput: boolean;
  outputChannels: number;
  uid: string;
}

export interface RuntimeGraphNode {
  control?: NodeControlState;
  config: unknown;
  id: string;
  label: string | null;
  type: string;
}

export interface NodeControlState {
  enabled: boolean;
  muted: boolean;
}

export interface NodeControlRequest {
  expectedRevision: number;
  enabled?: boolean;
  muted?: boolean;
  gainDb?: number;
}

export interface NodeControlResponse extends GraphApplyResponse {
  /** Native preparation may reconcile authored metadata outside the requested fields. */
  requiresGraphReload: boolean;
}

export interface GraphNodeControls {
  graphId: string;
  revision: number;
  nodes: {
    nodeId: string;
    label: string | null;
    kind: string;
    control: NodeControlState;
    capabilities: { power: boolean; mute: boolean; recorder: boolean };
    gainDb?: number;
  }[];
}

export interface RuntimeAudioEndpoint {
  nodeId: string;
  portId: string;
}

export interface RuntimeChannelRoute {
  destinationChannel: number;
  sourceChannel: number;
}

export interface RuntimeGraphEdge {
  channelMapping: { mode: 'auto' } | { mode: 'explicit'; routes: RuntimeChannelRoute[] };
  destination: RuntimeAudioEndpoint;
  id: string;
  source: RuntimeAudioEndpoint;
}

export interface RuntimeGraphDocument {
  edges: RuntimeGraphEdge[];
  nodes: RuntimeGraphNode[];
  revision: number;
  schemaVersion: 2;
}

export interface GraphRuntimeStatus {
  activeRevision: number;
  protocolVersion: 1;
  submittedRevision: number;
}

export interface NodeMeterSnapshot {
  nodeId: string;
  peaks: number[];
  portId: string;
  dynamics?: {
    inputPeaks: number[];
    gainReductionDb: number;
    thresholdDb: number;
  };
}

export interface MeterSnapshot {
  nodes: NodeMeterSnapshot[];
  protocolVersion: 1;
  revision: number;
}

/** One pushed meter window; the incarnation identifies the engine run that measured it. */
export interface MeterTelemetry {
  engineIncarnation: string;
  sequence: number;
  meters: MeterSnapshot;
}

/** Receives pushed meter windows until the stream ends. */
export interface MeterTelemetryListener {
  window(telemetry: MeterTelemetry): void;
  /** The engine or runtime ended the stream; subscribe again to resume. */
  ended(): void;
}

export interface ProcessorParameterDescriptor {
  canAutomate: boolean;
  defaultValue: number;
  id: number;
  isReadOnly: boolean;
  name: string;
  stepCount: number;
}

export type ProviderBusKind = 'main' | 'sidechain' | 'auxiliary';

export type ProviderChannelFormat = 'mono' | 'stereo' | 'surround5_1' | 'surround7_1' | 'discrete';

export interface ProviderBus {
  channelFormat: ProviderChannelFormat;
  channels: number;
  id: string;
  kind: ProviderBusKind;
  name: string;
}

export interface ProviderPorts {
  inputs: ProviderBus[];
  layoutId: string;
  outputs: ProviderBus[];
}

export interface ProviderNodeConfig {
  parameters?: Record<string, unknown>;
  ports?: ProviderPorts;
  /** The plug-in identity; layoutId is stored only inside ports. */
  typeId: string;
}

export interface ExternalProcessorDescriptor {
  displayName: string;
  format: 'au' | 'auv3' | 'clap' | 'lv2' | 'vst3';
  hasEditor: boolean;
  layouts: ProviderPorts[];
  defaultLayoutId: string;
  parameters: ProcessorParameterDescriptor[];
  typeId: string;
  vendor: string;
  version: string;
}

export interface ExternalProcessorDiagnostic {
  format: ExternalProcessorDescriptor['format'];
  stage: 'scan' | 'probe';
  pluginId?: string;
  pluginName?: string;
  message: string;
}

export interface ExternalProcessorCatalog {
  processors: ExternalProcessorDescriptor[];
  protocolVersion: 1;
  /** A partial native scan is not a successful empty catalog. */
  diagnostics?: ExternalProcessorDiagnostic[];
}

export interface ProcessorParameterUpdateResponse {
  nodeId: string;
  parameterId: number;
  protocolVersion: 1;
  value: number;
  /** Audio accepted the value, but durable graph storage may have failed. */
  persistenceError?: string;
}

export type ProcessorEditorState = 'unavailable' | 'closed' | 'opening' | 'open' | 'failed';

export type ProcessorEditorFailure =
  | 'no_editor'
  | 'display_unavailable'
  | 'host_window_failed'
  | 'plugin_rejected'
  | 'worker_unavailable';

export interface ProcessorEditorStatus {
  failure: ProcessorEditorFailure | null;
  height: number | null;
  nodeId: string;
  protocolVersion: 1;
  resizable: boolean;
  state: ProcessorEditorState;
  width: number | null;
}

export interface GraphApplyResponse {
  protocolVersion: 1;
  revision: number;
  state: 'active' | 'pending';
  persistenceError?: string;
}

export interface ProcessorRetryRequest {
  expectedCatalogRevision: number;
  expectedInstanceId: string;
  expectedRevision: number;
  graphId: string;
  nodeId: string;
}

/** Only controls explicitly edited by this client; never inferred from a stale engine snapshot. */
export interface ProcessorParameterChange {
  nodeId: string;
  typeId: string;
  parameterId: number;
  value: number;
}

export interface GraphApplyRequest extends RuntimeGraphDocument {
  processorParameterChanges?: ProcessorParameterChange[];
}

export type ProtocolErrorCode =
  | 'feature_limit'
  | 'node_not_found'
  | 'node_disabled'
  | 'recorder_failed'
  | 'audio_session_unsupported'
  | 'audio_session_processor_state_unavailable'
  | 'audio_session_reconfigure_failed'
  | 'audio_device_disconnected'
  | 'audio_source_unavailable'
  | 'control_unavailable'
  | 'control_request_failed'
  | 'engine_replaced'
  | 'invalid_operation_id'
  | 'graph_binding_unsupported'
  | 'graph_exchange_busy'
  | 'graph_invalid'
  | 'graph_revision_stale'
  | 'processor_instance_stale'
  | 'processor_not_failed'
  | 'processor_unavailable';

export interface ProtocolErrorBody {
  code: ProtocolErrorCode;
  parameters: unknown;
}

export class ControlPortError extends Error {
  public constructor(
    public readonly status: number,
    public readonly body: ProtocolErrorBody,
  ) {
    super(body.code);
    this.name = 'ControlPortError';
  }
}

export interface VirtualEndpointFormatRequest {
  endpointId: string;
  expectedRevision: number;
  sampleRate: number;
}

export type RecorderEncoding = 'pcm16' | 'pcm24' | 'float32';

export interface RecorderRequest {
  action: 'record' | 'pause' | 'stop';
}

export interface RecorderSettings {
  folder: string;
  sampleRate: number | null;
  encoding: RecorderEncoding;
}

export interface RecorderStatus {
  state: 'stopped' | 'recording' | 'paused' | 'failed';
  path: string | null;
  sampleRate: number | null;
  encoding: RecorderEncoding | null;
  frames: number;
  error: string | null;
}

/** Observed native device clients; flags describe OS playback/capture, not graph state. */
export interface DeviceAppActivity {
  bundleId: string;
  displayName: string;
  iconDataUrl?: string;
  processId: number;
  playing: boolean;
  recording: boolean;
}

export interface ControlPort {
  subscribeState?(listener: import('./telemetry').StateListener): () => void;
  /** A successful missing UID is observed empty; rejection means unavailable. */
  getVirtualDeviceAppActivity?(): Promise<Record<string, DeviceAppActivity[]>>;
  getNodeControls?(graphId: string): Promise<GraphNodeControls>;
  updateNodeControl?(
    graphId: string,
    nodeId: string,
    request: NodeControlRequest,
  ): Promise<NodeControlResponse>;
  workspaceFile?(request: {
    action: 'new' | 'open' | 'save' | 'save_as';
    expectedRevision: number;
    expectedCatalogRevision: number;
    locale: string;
  }): Promise<GraphCatalog | null>;
  onWorkspaceFile?(
    listener: (action: 'new' | 'open' | 'save' | 'save_as') => void,
  ): Promise<() => void>;
  getProcessorEditorFrame(nodeId: string): Promise<ProcessorEditorFrame>;
  sendProcessorEditorInput(nodeId: string, input: ProcessorEditorInput): Promise<void>;
  recorderStatus(graphId: string, nodeId: string): Promise<RecorderStatus>;
  controlRecorder(
    graphId: string,
    nodeId: string,
    request: RecorderRequest,
    expectedRevision: number,
  ): Promise<RecorderStatus>;
  recordingSettings(graphId: string, nodeId: string): Promise<RecorderSettings>;
  setRecordingSettings(
    graphId: string,
    nodeId: string,
    settings: RecorderSettings,
    expectedRevision: number,
  ): Promise<RecorderSettings>;
  selectRecordingFolder?(options: { defaultPath?: string; title?: string }): Promise<string | null>;
  setVirtualEndpointSampleRate?(request: VirtualEndpointFormatRequest): Promise<GraphApplyResponse>;
  getGraphLayout?(graphId: string): Promise<GraphLayoutResponse>;
  setGraphLayout?(graphId: string, request: GraphLayoutRequest): Promise<GraphLayoutResponse>;
  getGraphCatalog?(): Promise<GraphCatalog>;
  mutateGraphCatalog?(request: GraphCatalogRequest): Promise<GraphCatalog>;
  reconfigureSession?(request: {
    expectedRevision: number;
    format: AudioSessionStatus['format'];
  }): Promise<GraphApplyResponse>;
  applyGraph(document: GraphApplyRequest): Promise<GraphApplyResponse>;
  getGraph(): Promise<RuntimeGraphDocument>;
  getGraphStatus(): Promise<GraphRuntimeStatus>;
  getMeters(): Promise<MeterSnapshot>;
  /** Push alternative to polling `getMeters`; resolves to an unsubscribe function. */
  subscribeMeters?(listener: MeterTelemetryListener): Promise<() => void>;
  getProcessors(): Promise<ExternalProcessorCatalog>;
  getProcessorEditor(nodeId: string): Promise<ProcessorEditorStatus>;
  rescanProcessors(paths: string[]): Promise<ExternalProcessorCatalog>;
  getStatus(): Promise<MixRelayStatus>;
  setEnginePower?(enabled: boolean): Promise<GraphApplyResponse>;
  setProcessorParameter(
    nodeId: string,
    parameterId: number,
    value: number,
    expectedRevision: number,
  ): Promise<ProcessorParameterUpdateResponse>;
  /** Explicitly retries one failed processor instance; adapters must not retry this request. */
  retryProcessor?(request: ProcessorRetryRequest): Promise<GraphApplyResponse>;
  setProcessorEditor(
    nodeId: string,
    open: boolean,
    expectedRevision: number,
  ): Promise<ProcessorEditorStatus>;
  listCapturableApplications?(): Promise<CapturableApplication[]>;
  listAudioDevices?(): Promise<AudioDeviceDescriptor[]>;
  selectProcessorDirectory?(): Promise<string | null>;
}

export interface ProcessorEditorFrame {
  instanceId: string;
  session: number;
  width: number;
  height: number;
  pngBase64: string;
}

export interface ProcessorEditorInput {
  instanceId: string;
  session: number;
  kind: 'down' | 'move' | 'up' | 'cancel';
  x: number;
  y: number;
  modifiers: number;
}

export interface GraphCatalog {
  layout?: GraphLayout;
  layoutRevision?: number;
  protocolVersion: number;
  catalogRevision: number;
  activeId: string;
  graphs: { id: string; name: string }[];
  document: RuntimeGraphDocument;
  persistenceError?: string;
}

/** Presentation-only metadata. Saving it must never submit or reprepare the audio graph. */
export interface GraphLayout {
  /** Display-only labels keyed by UI instance, including split endpoint cards. */
  nodeLabels?: Record<string, string>;
  positions: Record<string, { x: number; y: number }>;
  sizes?: Record<string, { width: number; height: number }>;
  viewport?: { x: number; y: number; zoom: number };
}

export interface GraphLayoutResponse {
  graphId: string;
  layoutRevision: number;
  layout: GraphLayout;
}

export interface GraphLayoutRequest {
  expectedLayoutRevision: number;
  layout: GraphLayout;
}

export type GraphCatalogAction =
  | { action: 'create'; name: string }
  | { action: 'select'; id: string }
  | { action: 'delete'; id: string }
  | { action: 'rename'; id: string; name: string };

export type GraphCatalogRequest = GraphCatalogAction & {
  expectedRevision: number;
  expectedCatalogRevision: number;
};
