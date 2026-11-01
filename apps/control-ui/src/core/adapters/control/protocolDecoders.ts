import type { GraphCatalog, GraphLayout, GraphLayoutResponse } from '../../domain/control/catalog';
import type { ProtocolErrorBody } from '../../domain/control/errors';
import type {
  GraphApplyResponse,
  GraphNodeControls,
  GraphRuntimeStatus,
  NodeControlResponse,
  NodeControlState,
  RuntimeGraphDocument,
  RuntimeGraphEdge,
  RuntimeGraphNode,
} from '../../domain/control/graph';
import type {
  AudioDeviceDescriptor,
  CapturableApplication,
  DeviceAppActivity,
  RuntimeApplicationSelector,
} from '../../domain/control/inventory';
import type { MeterSnapshot, MeterTelemetry, NodeMeterSnapshot } from '../../domain/control/meters';
import type {
  NativeDeviceFormats,
  NativeStreamFormat,
  NativeStreamFormats,
} from '../../domain/control/nativeFormat';
import type {
  ExternalProcessorCatalog,
  ExternalProcessorDescriptor,
  ExternalProcessorDiagnostic,
  ProcessorEditorFrame,
  ProcessorEditorStatus,
  ProcessorParameterDescriptor,
  ProcessorParameterUpdateResponse,
  ProviderBus,
  ProviderPorts,
} from '../../domain/control/processors';
import type { RecorderSettings, RecorderStatus } from '../../domain/control/recorder';
import type { ControlStateSnapshot } from '../../domain/control/state';
import type {
  AudioSessionStatus,
  MixRelayStatus,
  RuntimeStreamSpec,
  VirtualEndpointDevice,
  VirtualEndpointSettings,
  VirtualEndpointSpec,
} from '../../domain/control/status';
import {
  array,
  boolean,
  type Decoder,
  integer,
  literal,
  nullable,
  number,
  object,
  optional,
  record,
  string,
  tagged,
  unknownValue,
} from '../decode';

// Every engine reply carries this protocol version; any other value is a different protocol.
const protocolVersion = literal(1);
const sampleFormat = literal('float32', 'signed16', 'signed24', 'signed32');

export const nativeStreamFormat = object<NativeStreamFormat>({
  sampleRate: integer,
  channels: integer,
  sampleFormat,
});
const nativeStreamFormats = object<NativeStreamFormats>({
  current: nullable(nativeStreamFormat),
  supported: array(nativeStreamFormat),
});
const nativeDeviceFormats = object<NativeDeviceFormats>({
  render: nativeStreamFormats,
  capture: nativeStreamFormats,
});

const runtimeStreamSpec = object<RuntimeStreamSpec>({
  channelLayout: literal('discrete', 'mono', 'stereo'),
  channels: integer,
});
const streamSpecFields = {
  render: optional(nullable(runtimeStreamSpec)),
  capture: optional(nullable(runtimeStreamSpec)),
  supportedSampleRates: array(integer),
  defaultSampleRate: integer,
};
export const virtualEndpointSpec = object<VirtualEndpointSpec>({
  id: string,
  displayName: string,
  ...streamSpecFields,
});
const virtualEndpointDevice = object<VirtualEndpointDevice>({
  nativeDeviceIds: array(string),
  nativeFormats: nativeDeviceFormats,
  spec: virtualEndpointSpec,
  observedSampleRate: nullable(integer),
  supportedSampleRates: array(integer),
  canSetSampleRate: boolean,
});
export const virtualEndpointSettings = object<VirtualEndpointSettings>({
  canCreate: boolean,
  canRename: boolean,
  renderChannels: array(integer),
  captureChannels: array(integer),
  creationTemplate: nullable(object(streamSpecFields)),
  devices: array(virtualEndpointDevice),
});

const sessionFormat = object<AudioSessionStatus['format']>({
  sampleRate: integer,
  blockFrames: integer,
});
const audioSessionStatus = object<AudioSessionStatus>({
  endpointPreferencesUnavailable: optional(boolean),
  virtualEndpointSettings: optional(virtualEndpointSettings),
  processingState: optional(literal('idle', 'running', 'stopped')),
  discontinuities: optional(integer),
  format: sessionFormat,
  preferredFormat: optional(nullable(sessionFormat)),
  clockAuthority: literal('engine_timer', 'device', 'audio_server', 'host'),
  observedDeviceSampleRate: nullable(integer),
  observedDeviceBufferFrames: optional(nullable(integer)),
  issue: nullable(
    literal(
      'processing_failed',
      'application_unavailable',
      'application_ambiguous',
      'device_unavailable',
      'sample_rate_mismatch',
      'block_size_mismatch',
      'clock_not_synchronized',
      'invalid_format',
      'reconfiguration_failed',
      'preferences_unavailable',
      'stored_format_unavailable',
    ),
  ),
  transportLatencyFrames: nullable(integer),
  capabilities: object<AudioSessionStatus['capabilities']>({
    backend: literal('core_audio', 'pipe_wire', 'contract_only'),
    supportedSampleRates: array(integer),
    supportedBlockFrames: array(integer),
    canReconfigureLive: boolean,
    hardwareInput: boolean,
    hardwareOutput: boolean,
    virtualEndpoint: boolean,
    applicationCapture: boolean,
    sampleRateConversion: boolean,
    driftCorrection: boolean,
  }),
});

export const mixRelayStatus = object<MixRelayStatus>({
  enginePower: optional(
    object<NonNullable<MixRelayStatus['enginePower']>>({
      enabled: boolean,
      transitioning: boolean,
      unsaved: optional(boolean),
      pauseReason: optional(literal('repeatedFailures')),
    }),
  ),
  failedProcessors: optional(
    array(object({ graphId: string, instanceId: string, nodeId: string })),
  ),
  processorTimings: optional(
    array(object({ graphId: string, nodeId: string, roundTripNs: integer })),
  ),
  processorLatencies: optional(
    array(
      object({
        graphId: string,
        nodeId: string,
        transportFrames: integer,
        intrinsicFrames: nullable(integer),
      }),
    ),
  ),
  recorderError: optional(nullable(string)),
  protocolVersion,
  driverState: literal(
    'not_installed',
    'installing',
    'restart_required',
    'installed_not_loaded',
    'blocked',
    'unavailable',
    'invalid_permissions',
    'version_mismatch',
    'ready',
  ),
  sampleRate: nullable(integer),
  bufferFrames: nullable(integer),
  underruns: integer,
  overruns: integer,
  audioSession: optional(audioSessionStatus),
});

const nodeControlState = object<NodeControlState>({ enabled: boolean, muted: boolean });
const runtimeGraphNode = object<RuntimeGraphNode>({
  control: optional(nodeControlState),
  config: unknownValue,
  id: string,
  label: nullable(string),
  type: string,
});
const endpoint = object({ nodeId: string, portId: string });
const runtimeGraphEdge = object<RuntimeGraphEdge>({
  channelMapping: tagged<RuntimeGraphEdge['channelMapping']>('mode', {
    auto: object({ mode: literal('auto') }),
    explicit: object({
      mode: literal('explicit'),
      routes: array(object({ destinationChannel: integer, sourceChannel: integer })),
    }),
  }),
  destination: endpoint,
  id: string,
  source: endpoint,
});
export const runtimeGraphDocument = object<RuntimeGraphDocument>({
  edges: array(runtimeGraphEdge),
  nodes: array(runtimeGraphNode),
  revision: integer,
  schemaVersion: literal(2),
});

export const graphRuntimeStatus = object<GraphRuntimeStatus>({
  pendingReason: optional(literal('awaiting_microphone_permission')),
  activeRevision: integer,
  protocolVersion,
  submittedRevision: integer,
});

const graphApplyFields = {
  protocolVersion,
  revision: integer,
  state: literal('active', 'pending'),
  persistenceError: optional(string),
};
export const graphApplyResponse = object<GraphApplyResponse>(graphApplyFields);
export const nodeControlResponse = object<NodeControlResponse>({
  ...graphApplyFields,
  requiresGraphReload: boolean,
});

const nodeControls = array(
  object<GraphNodeControls['nodes'][number]>({
    nodeId: string,
    label: nullable(string),
    kind: string,
    control: nodeControlState,
    capabilities: object({ power: boolean, mute: boolean, recorder: boolean }),
    gainDb: optional(number),
  }),
);
export const graphNodeControls = object<GraphNodeControls>({
  graphId: string,
  revision: integer,
  nodes: nodeControls,
});

const nodeMeterSnapshot = object<NodeMeterSnapshot>({
  nodeId: string,
  peaks: array(number),
  portId: string,
  dynamics: optional(
    object({ inputPeaks: array(number), gainReductionDb: number, thresholdDb: number }),
  ),
});
export const meterSnapshot = object<MeterSnapshot>({
  nodes: array(nodeMeterSnapshot),
  protocolVersion,
  revision: integer,
});
export const meterTelemetry = object<MeterTelemetry>({
  engineIncarnation: string,
  sequence: integer,
  meters: meterSnapshot,
});

const processorFormat = literal('au', 'auv3', 'clap', 'lv2', 'vst3');
const providerBus = object<ProviderBus>({
  channelFormat: literal('mono', 'stereo', 'surround5_1', 'surround7_1', 'discrete'),
  channels: integer,
  id: string,
  kind: literal('main', 'sidechain', 'auxiliary'),
  name: string,
});
const providerPorts = object<ProviderPorts>({
  inputs: array(providerBus),
  layoutId: string,
  outputs: array(providerBus),
});
const processorParameter = object<ProcessorParameterDescriptor>({
  canAutomate: boolean,
  defaultValue: number,
  id: integer,
  isReadOnly: boolean,
  name: string,
  stepCount: integer,
});
const processorDescriptor = object<ExternalProcessorDescriptor>({
  displayName: string,
  format: processorFormat,
  hasEditor: boolean,
  layouts: array(providerPorts),
  defaultLayoutId: string,
  parameters: array(processorParameter),
  typeId: string,
  vendor: string,
  version: string,
});
const processorDiagnostic = object<ExternalProcessorDiagnostic>({
  format: processorFormat,
  stage: literal('scan', 'probe'),
  pluginId: optional(string),
  pluginName: optional(string),
  message: string,
});
export const externalProcessorCatalog = object<ExternalProcessorCatalog>({
  processors: array(processorDescriptor),
  protocolVersion,
  diagnostics: optional(array(processorDiagnostic)),
});

export const processorParameterUpdateResponse = object<ProcessorParameterUpdateResponse>({
  nodeId: string,
  parameterId: integer,
  protocolVersion,
  value: number,
  persistenceError: optional(string),
});
export const processorEditorStatus = object<ProcessorEditorStatus>({
  failure: nullable(
    literal(
      'no_editor',
      'display_unavailable',
      'host_window_failed',
      'plugin_rejected',
      'worker_unavailable',
    ),
  ),
  height: nullable(integer),
  nodeId: string,
  protocolVersion,
  resizable: boolean,
  state: literal('unavailable', 'closed', 'opening', 'open', 'failed'),
  width: nullable(integer),
});
export const processorEditorFrame = object<ProcessorEditorFrame>({
  instanceId: string,
  session: integer,
  width: integer,
  height: integer,
  pngBase64: string,
});

const recorderEncoding = literal('pcm16', 'pcm24', 'float32');
export const recorderStatus = object<RecorderStatus>({
  state: literal('stopped', 'recording', 'paused', 'failed'),
  path: nullable(string),
  sampleRate: nullable(integer),
  encoding: nullable(recorderEncoding),
  frames: integer,
  error: nullable(string),
});
export const recorderSettings = object<RecorderSettings>({
  folder: string,
  sampleRate: nullable(integer),
  encoding: recorderEncoding,
});

const point = object({ x: number, y: number });
const graphLayout = object<GraphLayout>({
  nodeLabels: optional(record(string)),
  positions: record(point),
  sizes: optional(record(object({ width: number, height: number }))),
  viewport: optional(object({ x: number, y: number, zoom: number })),
});
export const graphLayoutResponse = object<GraphLayoutResponse>({
  graphId: string,
  layoutRevision: integer,
  layout: graphLayout,
});
export const graphCatalog = object<GraphCatalog>({
  layout: optional(graphLayout),
  layoutRevision: optional(integer),
  protocolVersion,
  catalogRevision: integer,
  activeId: string,
  graphs: array(object({ id: string, name: string })),
  document: runtimeGraphDocument,
  persistenceError: optional(string),
});

const selector: Decoder<RuntimeApplicationSelector> = tagged<RuntimeApplicationSelector>('match', {
  bundle_id: object({
    bundle_id: string,
    include_descendants: boolean,
    match: literal('bundle_id'),
  }),
  executable_path: object({ match: literal('executable_path'), path: string }),
  process_id: object({
    include_descendants: boolean,
    match: literal('process_id'),
    process_id: integer,
  }),
  all_except: object({ bundle_ids: array(string), match: literal('all_except') }),
});
export const capturableApplications = array(
  object<CapturableApplication>({
    bundleId: string,
    displayName: string,
    iconDataUrl: nullable(string),
    processId: integer,
    selector,
  }),
);
export const audioDevices = array(
  object<AudioDeviceDescriptor>({
    displayName: string,
    inputChannels: integer,
    isDefaultInput: boolean,
    isDefaultOutput: boolean,
    outputChannels: integer,
    uid: string,
  }),
);
export const deviceAppActivity = record(
  array(
    object<DeviceAppActivity>({
      bundleId: string,
      displayName: string,
      iconDataUrl: optional(string),
      processId: integer,
      playing: boolean,
      recording: boolean,
    }),
  ),
);

export const controlStateSnapshot = object<ControlStateSnapshot>({
  generation: integer,
  status: mixRelayStatus,
  graph: graphRuntimeStatus,
  controls: nullable(
    object<NonNullable<ControlStateSnapshot['controls']>>({
      revision: integer,
      activeRevision: integer,
      structureGeneration: integer,
      graphs: array(object({ id: string, nodes: nodeControls })),
    }),
  ),
  editors: array(processorEditorStatus),
});

/** Engine and desktop errors share this body. Codes are open-ended: an unrecognised code is
 * still a refusal, so only the shape is checked. */
export const protocolErrorBody = object<ProtocolErrorBody>({
  code: string,
  parameters: unknownValue,
});
