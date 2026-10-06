import type {
  AudioGraphModel,
  AudioGraphNode,
  AudioGraphPosition,
} from '../../core/audio-graph/model';
import { addAudioGraphNode } from '../../core/audio-graph/model';
import type {
  CapturableApplication,
  ExternalProcessorDescriptor,
  ProcessorParameterChange,
  ProviderBus,
  ProviderBusKind,
  ProviderChannelFormat,
  ProviderNodeConfig,
  ProviderPorts,
  RuntimeChannelRoute,
  RuntimeGraphDocument,
  RuntimeGraphEdge,
  RuntimeGraphNode,
  RuntimeStreamSpec,
  VirtualEndpointRateError,
  VirtualEndpointSettings,
  VirtualEndpointSpec,
} from '../../core/control-port/types';
import { applicationInventorySelection, applicationSelectorKey } from './applicationIdentity';
import { serializeGraphEdges } from './graphEdgeProjection';

const AUDIO_PORT_ID = 'audio';
const FROM_OS_PORT_ID = 'from_os';
const TO_OS_PORT_ID = 'to_os';
const SOURCE_NODE_SUFFIX = '::host-source';
const SINK_NODE_SUFFIX = '::host-sink';
const CHANNEL_PORT_SEPARATOR = '::channel:';

export interface GraphPresentation {
  application: string;
  audioInput: string;
  audioOutput: string;
  recorder: string;
  captureChannels: string;
  ceiling: string;
  compressor: string;
  deviceName: string;
  virtualSourcePort: string;
  gain: string;
  mixer: string;
  limiter: string;
  attack: string;
  makeupGain: string;
  options: string;
  outputGain: string;
  processor: string;
  processorLayoutUnavailable: string;
  providerBusKinds: Record<ProviderBusKind, string>;
  renderChannels: string;
  ratio: string;
  release: string;
  routing: string;
  sourceGain: string;
  virtualSinkPort: string;
  threshold: string;
  unsupported: string;
  utility: string;
  virtualInput: string;
  virtualOutput: string;
}

const defaultPresentation: GraphPresentation = {
  application: 'Application',
  audioInput: 'Default input',
  audioOutput: 'Default output',
  recorder: 'Recorder',
  captureChannels: 'Outputs',
  ceiling: 'Ceiling',
  compressor: 'Compressor',
  deviceName: 'Device name',
  virtualSourcePort: 'Audio from device',
  gain: 'Gain',
  mixer: 'Mixer',
  limiter: 'Limiter',
  attack: 'Attack',
  makeupGain: 'Makeup',
  options: 'Options',
  outputGain: 'Output gain',
  processor: 'Processor',
  processorLayoutUnavailable: 'Processor layout unavailable',
  providerBusKinds: { main: 'Main', sidechain: 'Sidechain', auxiliary: 'Auxiliary' },
  renderChannels: 'Inputs',
  ratio: 'Ratio',
  release: 'Release',
  routing: 'Routing',
  sourceGain: 'Source gain',
  virtualSinkPort: 'Audio to device',
  threshold: 'Threshold',
  unsupported: 'Unsupported',
  utility: 'Utility',
  virtualInput: 'Virtual input',
  virtualOutput: 'Virtual output',
};

export function runtimeGraphToAudioGraph(
  document: RuntimeGraphDocument,
  presentation: GraphPresentation = defaultPresentation,
  processors: readonly ExternalProcessorDescriptor[] = [],
): AudioGraphModel {
  const reservedIds = new Set(document.nodes.map((node) => node.id));
  const nodes = document.nodes.flatMap((node, index) =>
    runtimeNodeToAudioNodes(node, index, presentation, processors, reservedIds).map((view) => ({
      ...view,
      runtime: view.runtime ? { ...view.runtime, control: node.control } : undefined,
    })),
  );
  const projected = document.edges.map((edge) => ({
    edge,
    views: runtimeEdgeToAudioEdges(edge, nodes),
  }));
  return {
    edges: projected.flatMap(({ views }) => views),
    edgeProjections: projected,
    nodes,
    revision: document.revision,
  };
}

export function presentRoutingApplicationIcons(
  model: AudioGraphModel,
  applications: readonly CapturableApplication[],
): AudioGraphModel {
  const iconsBySelection = new Map<string, string | undefined>();
  for (const application of applications) {
    const selection = applicationInventorySelection(application);
    if (!selection) continue;
    const iconUrl = application.iconDataUrl || undefined;
    if (!iconsBySelection.has(selection.key) || (!iconsBySelection.get(selection.key) && iconUrl))
      iconsBySelection.set(selection.key, iconUrl);
  }
  let changed = false;
  const nodes = model.nodes.map((node) => {
    if (node.runtime?.type !== 'application_capture') return node;
    const config = objectValue(node.runtime.config);
    const selection =
      config?.requiresReselection === true ? undefined : applicationSelectorKey(config?.selector);
    const iconUrl = selection ? iconsBySelection.get(selection) : undefined;
    if (node.appearance.iconUrl === iconUrl) return node;
    changed = true;
    return { ...node, appearance: { ...node.appearance, iconUrl } };
  });
  return changed ? { ...model, nodes } : model;
}

export function audioGraphToRuntimeGraph(
  model: AudioGraphModel,
  source: RuntimeGraphDocument,
): RuntimeGraphDocument {
  const sourceNodes = new Map(source.nodes.map((node) => [node.id, node]));
  const includedNodes = new Map<string, RuntimeGraphNode>();
  for (const node of model.nodes) {
    const id = runtimeNodeId(node);
    const runtimeNode = audioNodeToRuntimeNode(node, model, sourceNodes.get(id));
    if (runtimeNode) includedNodes.set(id, runtimeNode);
  }
  return {
    edges: serializeGraphEdges(model, new Set(includedNodes.keys()), (edge) =>
      runtimeEdge(model, edge),
    ),
    nodes: [...includedNodes.values()],
    revision: model.revision,
    schemaVersion: 2,
  };
}

function audioNodeToRuntimeNode(
  node: AudioGraphNode,
  model: AudioGraphModel,
  sourceNode?: RuntimeGraphNode,
): RuntimeGraphNode | null {
  const id = runtimeNodeId(node);
  if (!node.runtime) return sourceNode ?? null;
  if (node.runtime.type !== 'virtual_endpoint') {
    return {
      ...(node.runtime.control ? { control: node.runtime.control } : {}),
      config: node.runtime.config,
      id,
      label: node.runtime.label === undefined ? node.appearance.title : node.runtime.label,
      type: node.runtime.type,
    };
  }
  const config = { ...(objectValue(node.runtime.config) ?? {}) };
  const endpointNodes = model.nodes.filter(
    (candidate) =>
      runtimeNodeId(candidate) === id && candidate.runtime?.type === 'virtual_endpoint',
  );
  // Both visible sides serialize into one endpoint. Its source owns the gain; do
  // not let the later sink card overwrite a live gain with its older config copy.
  const source = endpointNodes.find((candidate) => candidate.runtime?.binding?.side === 'source');
  const sourceGain = directNumber(source?.runtime?.config, 'gainDb');
  if (sourceGain !== null) config.gainDb = sourceGain;
  config.reference = {
    render: Boolean(source),
    capture: endpointNodes.some((candidate) => candidate.runtime?.binding?.side === 'sink'),
  };
  return {
    ...(node.runtime.control ? { control: node.runtime.control } : {}),
    config,
    id,
    label: sourceNode ? sourceNode.label : node.appearance.title,
    type: node.runtime.type,
  };
}

export type RoutingNodeRequest =
  | { kind: 'recorder' }
  | {
      bundleId: string;
      kind: 'application';
      selector?: CapturableApplication['selector'];
      title: string;
    }
  | { channels: number; kind: 'audio_input'; title: string; uid: string }
  | { channels: number; kind: 'audio_output'; title: string; uid: string }
  | { kind: 'compressor' }
  | { kind: 'limiter' }
  | { kind: 'provider'; ports: ProviderPorts; processor: ExternalProcessorDescriptor }
  | { kind: 'virtual_input'; spec: VirtualEndpointSpec }
  | { kind: 'virtual_output'; spec: VirtualEndpointSpec };

type MaterializedRoutingNodeRequest = Exclude<
  RoutingNodeRequest,
  { kind: 'virtual_input' } | { kind: 'virtual_output' }
>;

export function addRoutingNode(
  model: AudioGraphModel,
  request: RoutingNodeRequest,
  position: AudioGraphPosition,
  presentation: GraphPresentation = defaultPresentation,
  endpointSettings?: VirtualEndpointSettings,
): AudioGraphModel {
  if (request.kind === 'virtual_input' || request.kind === 'virtual_output') {
    if (
      endpointSettings &&
      !endpointSettings.canCreate &&
      !endpointSettings.devices.some((device) => device.spec.id === request.spec.id)
    )
      return model;
    return addVirtualEndpointSide(model, request, position, presentation);
  }
  const id = nextNodeId(model, request.kind);
  const base = {
    id,
    position,
  };
  const node = createRoutingNode(base, request, presentation);
  return addAudioGraphNode(model, node);
}

function addVirtualEndpointSide(
  model: AudioGraphModel,
  request: Extract<RoutingNodeRequest, { spec: VirtualEndpointSpec }>,
  position: AudioGraphPosition,
  presentation: GraphPresentation,
): AudioGraphModel {
  const { kind, spec } = request;
  const sibling = model.nodes.find(
    (node) =>
      node.runtime?.type === 'virtual_endpoint' &&
      directString(node.runtime.config, 'id') === spec.id,
  );
  const nodeId = sibling ? runtimeNodeId(sibling) : nextVirtualNodeId(model);
  const sideName = kind === 'virtual_input' ? 'source' : 'sink';
  if (
    model.nodes.some(
      (node) => node.runtime?.binding?.nodeId === nodeId && node.runtime.binding.side === sideName,
    )
  )
    return model;
  const siblingReference = objectValue(objectValue(sibling?.runtime?.config)?.reference);
  const config = {
    ...spec,
    gainDb: directNumber(sibling?.runtime?.config, 'gainDb') ?? 0,
    reference: {
      render: kind === 'virtual_input' || siblingReference?.render === true,
      capture: kind === 'virtual_output' || siblingReference?.capture === true,
    },
  };
  const endpoint: RuntimeGraphNode = {
    config,
    id: nodeId,
    label: spec.displayName,
    type: 'virtual_endpoint',
  };
  const side = virtualEndpointNodes(
    endpoint,
    presentation,
    new Set(model.nodes.map(runtimeNodeId)),
  ).find((node) => node.runtime?.binding?.side === sideName);
  return side ? addAudioGraphNode(model, { ...side, position }) : model;
}

function nextVirtualNodeId(model: AudioGraphModel): string {
  let sequence = 1;
  while (model.nodes.some((node) => runtimeNodeId(node) === `virtual-${sequence}`)) sequence += 1;
  return `virtual-${sequence}`;
}

export function updateRoutingNodeGain(
  model: AudioGraphModel,
  nodeId: string,
  gainDb: number,
): AudioGraphModel {
  if (!Number.isFinite(gainDb)) return model;
  const normalized = Math.max(-60, Math.min(12, Math.round(gainDb * 2) / 2));
  let changed = false;
  const nodes = model.nodes.map((node) => {
    const config = objectValue(node.runtime?.config);
    if (node.id !== nodeId || !node.appearance.gainControl || !node.runtime || !config) return node;
    if (config.gainDb === normalized) return node;
    changed = true;
    return {
      ...node,
      appearance: {
        ...node.appearance,
        detail:
          node.runtime.type === 'gain' ? `${normalized.toFixed(1)} dB` : node.appearance.detail,
        gainControl: { ...node.appearance.gainControl, valueDb: normalized },
      },
      runtime: { ...node.runtime, config: { ...config, gainDb: normalized } },
    };
  });
  return changed ? { ...model, nodes, revision: model.revision + 1 } : model;
}

/** Called only for explicit processor edits/Undo, never for a structural snapshot replay. */
export function changedProcessorParameters(
  before: AudioGraphModel,
  after: AudioGraphModel,
): ProcessorParameterChange[] {
  const beforeNodes = new Map(before.nodes.map((node) => [node.id, node]));
  return after.nodes.flatMap((node) => {
    const previous = beforeNodes.get(node.id);
    const typeId = directString(node.runtime?.config, 'typeId');
    if (
      node.runtime?.type !== 'provider' ||
      previous?.runtime?.type !== 'provider' ||
      !typeId ||
      directString(previous.runtime.config, 'typeId') !== typeId
    )
      return [];
    const oldControls = new Map(
      previous.appearance.parameterControls?.map((control) => [control.id, control.value]) ?? [],
    );
    return (node.appearance.parameterControls ?? []).flatMap((control) => {
      if (!control.id.startsWith('external:') || oldControls.get(control.id) === control.value)
        return [];
      const parameterId = Number(control.id.slice(9));
      return Number.isInteger(parameterId) && parameterId >= 0
        ? [{ nodeId: node.id, typeId, parameterId, value: control.value }]
        : [];
    });
  });
}

export function updateRoutingNodeParameter(
  model: AudioGraphModel,
  nodeId: string,
  parameterId: string,
  value: number,
): AudioGraphModel {
  if (!Number.isFinite(value)) return model;
  let changed = false;
  const nodes = model.nodes.map((node) => {
    const control = node.appearance.parameterControls?.find(
      (candidate) => candidate.id === parameterId,
    );
    const config = objectValue(node.runtime?.config);
    if (node.id !== nodeId || !node.runtime || !config || !control) return node;
    const stepped = Math.round(value / control.step) * control.step;
    const normalized = Math.max(control.minimum, Math.min(control.maximum, stepped));
    if (node.runtime.type === 'provider') {
      const id = parameterId.startsWith('external:') ? parameterId.slice(9) : parameterId;
      const parameters = objectValue(config.parameters) ?? {};
      const values = objectValue(parameters.parameterValues) ?? {};
      if (values[id] === normalized) return node;
      changed = true;
      return {
        ...node,
        appearance: {
          ...node.appearance,
          parameterControls: node.appearance.parameterControls?.map((candidate) =>
            candidate.id === parameterId ? { ...candidate, value: normalized } : candidate,
          ),
        },
        runtime: {
          ...node.runtime,
          config: {
            ...config,
            parameters: { ...parameters, parameterValues: { ...values, [id]: normalized } },
          },
        },
      };
    }
    if (config[parameterId] === normalized) return node;
    changed = true;
    const nextConfig = { ...config, [parameterId]: normalized };
    return {
      ...node,
      appearance: {
        ...node.appearance,
        detail: processorDetail(node.runtime.type, nextConfig),
        parameterControls: node.appearance.parameterControls?.map((candidate) =>
          candidate.id === parameterId ? { ...candidate, value: normalized } : candidate,
        ),
      },
      runtime: { ...node.runtime, config: nextConfig },
    };
  });
  return changed ? { ...model, nodes, revision: model.revision + 1 } : model;
}

export interface VirtualEndpointUpdate {
  nativeFormat?: import('../../core/ui/AudioNodeCard').EndpointUpdate['nativeFormat'];
  /** Dedicated native-device transaction; never serialized by updateVirtualEndpoint. */
  sampleRate?: number;
  captureChannels: number;
  displayName: string;
  renderChannels: number;
}

export function updateVirtualEndpoint(
  model: AudioGraphModel,
  nodeId: string,
  update: VirtualEndpointUpdate,
  presentation: GraphPresentation = defaultPresentation,
  settings?: VirtualEndpointSettings,
): AudioGraphModel {
  const selected = model.nodes.find((node) => node.id === nodeId);
  if (!selected) return model;
  const endpointNodeId = runtimeNodeId(selected);
  const displayName = update.displayName.trim();
  if (!displayName) return model;
  const current = model.nodes.find(
    (node) => runtimeNodeId(node) === endpointNodeId && node.runtime?.type === 'virtual_endpoint',
  );
  const currentConfig = objectValue(current?.runtime?.config);
  if (!current || !currentConfig) return model;
  const channels = editedEndpointChannels(currentConfig, update);
  if (!channels) return model;
  const { renderChannels, captureChannels } = channels;
  const deviceId = directString(currentConfig, 'id');
  if (!deviceId) return model;
  if (
    !permitsEndpointEdit(
      settings,
      renderChannels,
      captureChannels,
      displayName === directString(currentConfig, 'displayName'),
    )
  )
    return model;
  if (
    (nestedNumber(currentConfig, 'render', 'channels') ?? 0) === renderChannels &&
    (nestedNumber(currentConfig, 'capture', 'channels') ?? 0) === captureChannels &&
    (directString(currentConfig, 'displayName') ?? current.appearance.title) === displayName
  ) {
    return model;
  }
  const value = { captureChannels, displayName, renderChannels };
  const nodes = model.nodes.map((node) =>
    node.runtime?.type === 'virtual_endpoint' &&
    directString(node.runtime.config, 'id') === deviceId
      ? updatedVirtualEndpointNode(node, value, presentation)
      : node,
  );
  return updateNodeGeometry(model, nodes);
}

/** Channel edits are graph changes; an active take is guarded by the recorder control boundary. */
export function updateRecorderChannels(
  model: AudioGraphModel,
  nodeId: string,
  channels: number,
): AudioGraphModel {
  if (!Number.isInteger(channels) || channels < 1 || channels > 8) return model;
  const selected = model.nodes.find((node) => node.id === nodeId);
  if (
    selected?.runtime?.type !== 'recorder' ||
    directNumber(selected.runtime.config, 'channels') === channels
  )
    return model;
  const nodes = model.nodes.map((node) =>
    node === selected
      ? {
          ...node,
          appearance: {
            ...node.appearance,
            channelCount: channels,
            meterBanks: [{ channels, nodeId: runtimeNodeId(node), portId: AUDIO_PORT_ID }],
          },
          inputPorts: channelPorts(AUDIO_PORT_ID, channels, node.appearance.title),
          runtime: { ...selected.runtime, type: 'recorder', config: { channels } },
        }
      : node,
  );
  return updateNodeGeometry(model, nodes);
}

function updateNodeGeometry(model: AudioGraphModel, nodes: AudioGraphNode[]): AudioGraphModel {
  const validNodePorts = new Map(
    nodes.map((node) => [
      node.id,
      {
        inputs: new Set(node.inputPorts.map((port) => port.id)),
        outputs: new Set(node.outputPorts.map((port) => port.id)),
      },
    ]),
  );
  const edges = model.edges.filter((edge) => {
    const source = validNodePorts.get(edge.sourceNodeId);
    const target = validNodePorts.get(edge.targetNodeId);
    return source?.outputs.has(edge.sourcePortId) && target?.inputs.has(edge.targetPortId);
  });
  return { ...model, edges, nodes, revision: model.revision + 1 };
}

/** Apply polled control policy only to presentation, never to the runtime document. */
export function presentVirtualEndpointSettings(
  model: AudioGraphModel,
  settings?: VirtualEndpointSettings,
  sampleRateLabel = 'Sample rate',
  rateAction?: { enabled: boolean; error: VirtualEndpointRateError | null },
): AudioGraphModel {
  if (!settings) return model;
  return {
    ...model,
    nodes: model.nodes.map((node) =>
      !node.appearance.endpointControl
        ? node
        : {
            ...node,
            appearance: {
              ...node.appearance,
              endpointControl: {
                ...node.appearance.endpointControl,
                ...presentEndpointRate(
                  virtualEndpointIdentity(node),
                  settings,
                  sampleRateLabel,
                  rateAction,
                ),
                canRename: settings.canRename,
                allowedRenderChannels: node.appearance.endpointControl.renderChannels
                  ? settings.renderChannels
                  : [],
                allowedCaptureChannels: node.appearance.endpointControl.captureChannels
                  ? settings.captureChannels
                  : [],
              },
            },
          },
    ),
  };
}

function presentEndpointRate(
  endpointId: string | null,
  settings: VirtualEndpointSettings,
  label: string,
  action?: { enabled: boolean; error: VirtualEndpointRateError | null },
) {
  const device = settings.devices.find((candidate) => candidate.spec.id === endpointId);
  const rate = device?.observedSampleRate;
  const observed = typeof rate === 'number' && Number.isFinite(rate) && rate > 0;
  return {
    observedRate: observed ? { label, value: rate } : undefined,
    sampleRateOptions:
      observed && action?.enabled && device?.canSetSampleRate
        ? device.supportedSampleRates
        : undefined,
    rateError: action?.error?.endpointId === endpointId ? action.error.message : null,
  };
}

function updatedVirtualEndpointNode(
  node: AudioGraphNode,
  update: VirtualEndpointUpdate,
  presentation: GraphPresentation,
): AudioGraphNode {
  const config = objectValue(node.runtime?.config) ?? {};
  const isSourceNode = node.runtime?.binding?.side === 'source';
  const channelCount = isSourceNode ? update.captureChannels : update.renderChannels;
  const runtimePortId = isSourceNode ? FROM_OS_PORT_ID : TO_OS_PORT_ID;
  const label = isSourceNode ? presentation.virtualSourcePort : presentation.virtualSinkPort;
  const boundary = isSourceNode ? 'source' : 'sink';
  const ports = channelPorts(runtimePortId, channelCount, label, boundary);
  return {
    ...node,
    appearance: {
      ...node.appearance,
      channelCount,
      endpointControl: endpointControl(
        update.displayName,
        update.renderChannels,
        update.captureChannels,
        presentation,
      ),
      meterBanks: [{ channels: channelCount, nodeId: runtimeNodeId(node), portId: runtimePortId }],
      title: update.displayName,
    },
    inputPorts: isSourceNode ? [] : ports,
    outputPorts: isSourceNode ? ports : [],
    runtime: {
      ...node.runtime,
      config: updatedVirtualDeviceConfig(config, update),
      type: 'virtual_endpoint',
    },
  };
}

function createRoutingNode(
  base: { id: string; position: AudioGraphPosition },
  request: MaterializedRoutingNodeRequest,
  presentation: GraphPresentation,
): AudioGraphNode {
  if (request.kind === 'application') return applicationNode(base, request, presentation);
  if (request.kind === 'audio_input') return audioInputNode(base, request, presentation);
  if (request.kind === 'audio_output') return audioOutputNode(base, request, presentation);
  if (request.kind === 'recorder') return recorderNode(base, 2, presentation);
  if (request.kind === 'compressor') return compressorNode(base, presentation);
  if (request.kind === 'limiter') return limiterNode(base, presentation);
  return externalProcessorNode(base, request.processor, request.ports, presentation);
}

function compressorNode(
  base: { id: string; position: AudioGraphPosition },
  presentation: GraphPresentation,
): AudioGraphNode {
  const config = {
    attackMs: 10,
    channels: 2,
    makeupGainDb: 0,
    ratio: 4,
    releaseMs: 120,
    thresholdDb: -18,
    version: 1,
  };
  return processorNode(
    base,
    'compressor',
    presentation.compressor,
    'COMP',
    config,
    compressorControls(config, presentation),
    presentation,
  );
}

function limiterNode(
  base: { id: string; position: AudioGraphPosition },
  presentation: GraphPresentation,
): AudioGraphNode {
  const config = { ceilingDb: -1, channels: 2, releaseMs: 80, version: 1 };
  return processorNode(
    base,
    'limiter',
    presentation.limiter,
    'LIM',
    config,
    limiterControls(config, presentation),
    presentation,
  );
}

function externalProcessorNode(
  base: { id: string; position: AudioGraphPosition },
  processor: ExternalProcessorDescriptor,
  ports: ProviderPorts,
  presentation: GraphPresentation,
): AudioGraphNode {
  const parameterValues = Object.fromEntries(
    processor.parameters.map((parameter) => [String(parameter.id), parameter.defaultValue]),
  );
  return {
    ...base,
    appearance: {
      category: `${presentation.processor} · ${processor.format.toUpperCase()}`,
      channelCount: totalBusChannels(ports.outputs),
      icon: processor.format.toUpperCase(),
      hasProcessorEditor: processor.hasEditor,
      meterBanks: ports.outputs.map((bus) => ({
        channels: bus.channels,
        label: providerBusLabel(bus, presentation),
        nodeId: base.id,
        portId: bus.id,
      })),
      optionsLabel: presentation.options,
      parameterControls: externalParameterControls(processor, parameterValues),
      title: processor.displayName,
    },
    inputPorts: providerChannelPorts(ports.inputs, presentation),
    outputPorts: providerChannelPorts(ports.outputs, presentation),
    runtime: {
      config: {
        parameters: { parameterValues, version: 1 },
        ports,
        typeId: processor.typeId,
      } satisfies ProviderNodeConfig,
      type: 'provider',
    },
  };
}

function providerChannelPorts(buses: readonly ProviderBus[], presentation: GraphPresentation) {
  return buses.flatMap((bus) =>
    channelPorts(bus.id, bus.channels, providerBusLabel(bus, presentation)),
  );
}

function providerBusLabel(bus: ProviderBus, presentation: GraphPresentation) {
  const kind = presentation.providerBusKinds[bus.kind];
  return bus.name ? `${kind} · ${bus.name}` : kind;
}

export function providerPortsFrom(value: unknown): ProviderPorts | null {
  const config = objectValue(value);
  if (
    !config ||
    !hasOnlyKeys(config, ['layoutId', 'inputs', 'outputs']) ||
    typeof config.layoutId !== 'string' ||
    !validProviderIdentity(config.layoutId, 256) ||
    !Array.isArray(config.inputs) ||
    !Array.isArray(config.outputs) ||
    config.inputs.length === 0 ||
    config.outputs.length === 0 ||
    config.inputs.length > 64 ||
    config.outputs.length > 64
  )
    return null;
  const inputs = config.inputs.map(providerBusFrom);
  const outputs = config.outputs.map(providerBusFrom);
  if (inputs.some((bus) => bus === null) || outputs.some((bus) => bus === null)) return null;
  const validInputs = inputs as ProviderBus[];
  const validOutputs = outputs as ProviderBus[];
  if (
    totalBusChannels(validInputs) > 64 ||
    totalBusChannels(validOutputs) > 64 ||
    !uniqueBusIds(validInputs) ||
    !uniqueBusIds(validOutputs)
  )
    return null;
  return { inputs: validInputs, layoutId: config.layoutId, outputs: validOutputs };
}

function providerBusFrom(value: unknown): ProviderBus | null {
  const bus = objectValue(value);
  if (
    !bus ||
    !hasOnlyKeys(bus, ['id', 'name', 'kind', 'channels', 'channelFormat']) ||
    typeof bus.id !== 'string' ||
    typeof bus.name !== 'string' ||
    !validProviderIdentity(bus.id, 128) ||
    byteLength(bus.name) > 1_024 ||
    bus.name.includes('\0') ||
    !['main', 'sidechain', 'auxiliary'].includes(String(bus.kind)) ||
    !['mono', 'stereo', 'surround5_1', 'surround7_1', 'discrete'].includes(
      String(bus.channelFormat),
    ) ||
    typeof bus.channels !== 'number' ||
    !Number.isInteger(bus.channels) ||
    bus.channels < 1 ||
    bus.channels > 64
  )
    return null;
  const expectedChannels = {
    mono: 1,
    stereo: 2,
    surround5_1: 6,
    surround7_1: 8,
  }[String(bus.channelFormat) as 'mono' | 'stereo' | 'surround5_1' | 'surround7_1'];
  if (expectedChannels !== undefined && bus.channels !== expectedChannels) return null;
  return {
    channelFormat: bus.channelFormat as ProviderChannelFormat,
    channels: bus.channels,
    id: bus.id,
    kind: bus.kind as ProviderBusKind,
    name: bus.name,
  };
}

function uniqueBusIds(buses: readonly ProviderBus[]) {
  const ids = buses.map((bus) => bus.id);
  return ids.every((id) => id.length > 0) && new Set(ids).size === ids.length;
}

function validProviderIdentity(value: string, maximumBytes: number) {
  return value.length > 0 && byteLength(value) <= maximumBytes && !/\p{Cc}/u.test(value);
}

function byteLength(value: string) {
  return new TextEncoder().encode(value).length;
}

function hasOnlyKeys(value: Record<string, unknown>, keys: readonly string[]) {
  return Object.keys(value).every((key) => keys.includes(key));
}

function providerPortsMatch(left: ProviderPorts, right: ProviderPorts) {
  const sameBuses = (a: readonly ProviderBus[], b: readonly ProviderBus[]) =>
    a.length === b.length &&
    a.every((bus, index) => {
      const other = b[index];
      return (
        other !== undefined &&
        bus.id === other.id &&
        bus.kind === other.kind &&
        bus.channels === other.channels &&
        bus.channelFormat === other.channelFormat
      );
    });
  return (
    left.layoutId === right.layoutId &&
    sameBuses(left.inputs, right.inputs) &&
    sameBuses(left.outputs, right.outputs)
  );
}

function totalBusChannels(buses: readonly ProviderBus[]) {
  return buses.reduce((total, bus) => total + bus.channels, 0);
}

function externalParameterControls(
  processor: ExternalProcessorDescriptor,
  values: Record<string, number>,
): NonNullable<AudioGraphNode['appearance']['parameterControls']> {
  return processor.parameters
    .filter((parameter) => !parameter.isReadOnly)
    .map((parameter) => ({
      id: `external:${parameter.id}`,
      label: parameter.name,
      maximum: 1,
      minimum: 0,
      step: parameter.stepCount > 0 ? 1 / parameter.stepCount : 0.001,
      suffix: '',
      value: values[String(parameter.id)] ?? parameter.defaultValue,
    }));
}

function processorNode(
  base: { id: string; position: AudioGraphPosition },
  type: 'compressor' | 'limiter',
  title: string,
  icon: string,
  config: Record<string, unknown>,
  parameterControls: NonNullable<AudioGraphNode['appearance']['parameterControls']>,
  presentation: GraphPresentation,
): AudioGraphNode {
  const channels = directNumber(config, 'channels') ?? 2;
  return {
    ...base,
    appearance: {
      category: presentation.processor,
      channelCount: channels,
      detail: processorDetail(type, config),
      icon,
      meterBanks: [{ channels, nodeId: base.id, portId: AUDIO_PORT_ID }],
      optionsLabel: presentation.options,
      parameterControls,
      title,
    },
    inputPorts: channelPorts(AUDIO_PORT_ID, channels, `${title} input`),
    outputPorts: channelPorts(AUDIO_PORT_ID, channels, `${title} output`),
    runtime: { config, type },
  };
}

function compressorControls(
  config: Record<string, unknown>,
  presentation: GraphPresentation,
): NonNullable<AudioGraphNode['appearance']['parameterControls']> {
  return [
    parameterControl('thresholdDb', presentation.threshold, -60, 0, 0.5, 'dB', config),
    parameterControl('ratio', presentation.ratio, 1, 20, 0.5, ':1', config),
    parameterControl('attackMs', presentation.attack, 0.1, 200, 0.5, 'ms', config),
    parameterControl('releaseMs', presentation.release, 5, 2_000, 5, 'ms', config),
    parameterControl('makeupGainDb', presentation.makeupGain, 0, 24, 0.5, 'dB', config),
  ];
}

function limiterControls(
  config: Record<string, unknown>,
  presentation: GraphPresentation,
): NonNullable<AudioGraphNode['appearance']['parameterControls']> {
  return [
    parameterControl('ceilingDb', presentation.ceiling, -24, 0, 0.5, 'dB', config),
    parameterControl('releaseMs', presentation.release, 5, 2_000, 5, 'ms', config),
  ];
}

function parameterControl(
  id: string,
  label: string,
  minimum: number,
  maximum: number,
  step: number,
  suffix: string,
  config: Record<string, unknown>,
) {
  return {
    id,
    label,
    maximum,
    minimum,
    step,
    suffix,
    value: directNumber(config, id) ?? minimum,
  };
}

function processorDetail(type: string, config: Record<string, unknown>): string | undefined {
  if (type === 'compressor') {
    const threshold = directNumber(config, 'thresholdDb') ?? -18;
    const ratio = directNumber(config, 'ratio') ?? 4;
    return `${threshold.toFixed(1)} dB · ${ratio.toFixed(1)}:1`;
  }
  if (type === 'limiter') {
    const ceiling = directNumber(config, 'ceilingDb') ?? -1;
    return `${ceiling.toFixed(1)} dB ceiling`;
  }
  return undefined;
}

function applicationNode(
  base: { id: string; position: AudioGraphPosition },
  request: Extract<RoutingNodeRequest, { kind: 'application' }>,
  presentation: GraphPresentation,
): AudioGraphNode {
  return {
    ...base,
    appearance: {
      category: presentation.application,
      channelCount: 2,
      gainControl: { label: presentation.sourceGain, valueDb: 0 },
      icon: 'APP',
      meterBanks: [{ channels: 2, nodeId: base.id, portId: AUDIO_PORT_ID }],
      optionsLabel: presentation.options,
      title: request.title,
    },
    inputPorts: [],
    outputPorts: channelPorts(AUDIO_PORT_ID, 2, 'Application audio'),
    runtime: {
      config: {
        gainDb: 0,
        selector: request.selector ?? {
          bundle_id: request.bundleId,
          include_descendants: true,
          match: 'bundle_id',
        },
        sourcePlayback: 'preserve',
        stream: { channelLayout: 'stereo', channels: 2 },
      },
      type: 'application_capture',
    },
  };
}

function audioInputNode(
  base: { id: string; position: AudioGraphPosition },
  request: Extract<RoutingNodeRequest, { kind: 'audio_input' }>,
  presentation: GraphPresentation,
): AudioGraphNode {
  return {
    ...base,
    appearance: {
      category: presentation.audioInput,
      channelCount: request.channels,
      gainControl: { label: presentation.sourceGain, valueDb: 0 },
      icon: 'ADC',
      meterBanks: [{ channels: request.channels, nodeId: base.id, portId: AUDIO_PORT_ID }],
      optionsLabel: presentation.options,
      title: request.title,
    },
    inputPorts: [],
    outputPorts: channelPorts(AUDIO_PORT_ID, request.channels, 'Device input'),
    runtime: {
      config: {
        gainDb: 0,
        outputChannels: request.channels,
        selector: { match: 'persistent_uid', uid: request.uid },
      },
      type: 'audio_device_input',
    },
  };
}

function audioOutputNode(
  base: { id: string; position: AudioGraphPosition },
  request: Extract<RoutingNodeRequest, { kind: 'audio_output' }>,
  presentation: GraphPresentation,
): AudioGraphNode {
  return {
    ...base,
    appearance: {
      category: presentation.audioOutput,
      channelCount: request.channels,
      icon: 'DAC',
      meterBanks: [{ channels: request.channels, nodeId: base.id, portId: AUDIO_PORT_ID }],
      title: request.title,
    },
    inputPorts: channelPorts(AUDIO_PORT_ID, request.channels, 'Device output'),
    outputPorts: [],
    runtime: {
      config: {
        inputChannels: request.channels,
        selector: { match: 'persistent_uid', uid: request.uid },
      },
      type: 'audio_device_output',
    },
  };
}

function recorderNode(
  base: { id: string; position: AudioGraphPosition },
  channels: number,
  presentation: GraphPresentation,
): AudioGraphNode {
  return {
    ...base,
    appearance: {
      category: presentation.recorder,
      title: presentation.recorder,
      icon: 'REC',
      channelCount: channels,
      meterBanks: [{ channels, nodeId: base.id, portId: AUDIO_PORT_ID }],
    },
    inputPorts: channelPorts(AUDIO_PORT_ID, channels, presentation.recorder),
    outputPorts: [],
    runtime: { type: 'recorder', config: { channels } },
  };
}

function nextNodeId(model: AudioGraphModel, kind: RoutingNodeRequest['kind']): string {
  let sequence = 1;
  while (model.nodes.some((node) => node.id === `${kind}-${sequence}`)) sequence += 1;
  return `${kind}-${sequence}`;
}

function runtimeNodeToAudioNodes(
  node: RuntimeGraphNode,
  index: number,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
  reservedIds: ReadonlySet<string>,
): AudioGraphNode[] {
  if (node.type === 'virtual_endpoint')
    return virtualEndpointNodes(node, presentation, reservedIds);
  const base = {
    id: node.id,
    position: runtimeNodePosition(node.type, index),
  };
  const runtime = { config: node.config, type: node.type };
  if (node.type === 'recorder') {
    const parsed = recorderNode(base, directNumber(node.config, 'channels') ?? 2, presentation);
    parsed.runtime = runtime;
    parsed.appearance.title = node.label ?? presentation.recorder;
    return [parsed];
  }
  const knownNode =
    runtimeSourceNode(node, base, runtime, presentation) ??
    runtimeProcessorNode(node, base, runtime, presentation, processors);
  if (knownNode) return [knownNode];
  return [unsupportedRuntimeNode(node, base, presentation)];
}

function runtimeSourceNode(
  node: RuntimeGraphNode,
  base: { id: string; position: AudioGraphPosition },
  runtime: NonNullable<AudioGraphNode['runtime']>,
  presentation: GraphPresentation,
): AudioGraphNode | null {
  if (node.type === 'application_capture') {
    const channels = nestedNumber(node.config, 'stream', 'channels') ?? 2;
    const parsed = applicationNode(
      base,
      {
        bundleId: nestedString(node.config, 'selector', 'bundle_id') ?? '',
        kind: 'application',
        title: node.label ?? node.id,
      },
      presentation,
    );
    parsed.appearance.channelCount = channels;
    parsed.appearance.meterBanks = [{ channels, nodeId: node.id, portId: AUDIO_PORT_ID }];
    parsed.outputPorts = channelPorts(AUDIO_PORT_ID, channels, 'Application audio');
    parsed.appearance.gainControl = {
      label: presentation.sourceGain,
      valueDb: directNumber(node.config, 'gainDb') ?? 0,
    };
    parsed.runtime = runtime;
    return parsed;
  }
  if (node.type === 'audio_device_input') {
    const parsed = audioInputNode(
      base,
      {
        channels: directNumber(node.config, 'outputChannels') ?? 2,
        kind: 'audio_input',
        title: node.label ?? node.id,
        uid: nestedString(node.config, 'selector', 'uid') ?? '',
      },
      presentation,
    );
    parsed.appearance.gainControl = {
      label: presentation.sourceGain,
      valueDb: directNumber(node.config, 'gainDb') ?? 0,
    };
    parsed.runtime = runtime;
    return parsed;
  }
  if (node.type === 'audio_device_output') {
    const parsed = audioOutputNode(
      base,
      {
        channels: directNumber(node.config, 'inputChannels') ?? 2,
        kind: 'audio_output',
        title: node.label ?? node.id,
        uid: nestedString(node.config, 'selector', 'uid') ?? '',
      },
      presentation,
    );
    parsed.runtime = runtime;
    return parsed;
  }
  return null;
}

function runtimeProcessorNode(
  node: RuntimeGraphNode,
  base: { id: string; position: AudioGraphPosition },
  runtime: NonNullable<AudioGraphNode['runtime']>,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
): AudioGraphNode | null {
  const dynamics = runtimeDynamicsNode(node, base, runtime, presentation);
  if (dynamics) return dynamics;
  if (node.type === 'provider')
    return runtimeExternalProcessorNode(node, base, runtime, presentation, processors);
  if (node.type === 'gain') {
    const gainDb = gainValue(node.config);
    return {
      ...base,
      appearance: {
        category: presentation.utility,
        channelCount: directNumber(node.config, 'channels') ?? 2,
        detail: `${gainDb.toFixed(1)} dB`,
        gainControl: { label: presentation.gain, valueDb: gainDb },
        icon: 'GAIN',
        meterBanks: [
          {
            channels: directNumber(node.config, 'channels') ?? 2,
            nodeId: node.id,
            portId: AUDIO_PORT_ID,
          },
        ],
        optionsLabel: presentation.options,
        title: node.label ?? node.id,
      },
      inputPorts: channelPorts(
        AUDIO_PORT_ID,
        directNumber(node.config, 'channels') ?? 2,
        'Gain input',
      ),
      outputPorts: channelPorts(
        AUDIO_PORT_ID,
        directNumber(node.config, 'channels') ?? 2,
        'Gain output',
      ),
      runtime,
    };
  }
  if (node.type === 'mixer') {
    const inputs = directStringArray(node.config, 'inputs');
    const channels = directNumber(node.config, 'channels') ?? 2;
    return {
      ...base,
      appearance: {
        category: presentation.routing,
        channelCount: channels,
        icon: 'MIX',
        meterBanks: [
          {
            channels: directNumber(node.config, 'channels') ?? 2,
            nodeId: node.id,
            portId: AUDIO_PORT_ID,
          },
        ],
        title: node.label ?? presentation.mixer,
      },
      inputPorts: inputs.flatMap((id, inputIndex) =>
        channelPorts(id, channels, `Mixer input ${inputIndex + 1}`),
      ),
      outputPorts: channelPorts(AUDIO_PORT_ID, channels, 'Mixer output'),
      runtime,
    };
  }
  return null;
}

function runtimeExternalProcessorNode(
  node: RuntimeGraphNode,
  base: { id: string; position: AudioGraphPosition },
  runtime: NonNullable<AudioGraphNode['runtime']>,
  presentation: GraphPresentation,
  processors: readonly ExternalProcessorDescriptor[],
): AudioGraphNode | null {
  const typeId = directString(node.config, 'typeId');
  const descriptor = processors.find((processor) => processor.typeId === typeId);
  const config = objectValue(node.config) ?? {};
  const hasAuthoredPorts = config.ports !== null && config.ports !== undefined;
  const authoredPorts = providerPortsFrom(config.ports);
  if (!descriptor) {
    return {
      ...base,
      runtime: { ...runtime, label: node.label },
      appearance: {
        title: node.label ?? typeId ?? node.id,
        category: presentation.processor,
        icon: 'FX',
        meterBanks: (authoredPorts?.outputs ?? []).map((bus) => ({
          channels: bus.channels,
          label: providerBusLabel(bus, presentation),
          nodeId: base.id,
          portId: bus.id,
        })),
        offline: true,
        detail: presentation.unsupported,
      },
      inputPorts: authoredPorts ? providerChannelPorts(authoredPorts.inputs, presentation) : [],
      outputPorts: authoredPorts ? providerChannelPorts(authoredPorts.outputs, presentation) : [],
    };
  }
  const layout = resolveProcessorLayout(descriptor, authoredPorts, hasAuthoredPorts);
  const parameters = config.parameters;
  const parameterValues = objectValue(objectValue(parameters)?.parameterValues) ?? {};
  const values = Object.fromEntries(
    Object.entries(parameterValues).filter(
      (entry): entry is [string, number] => typeof entry[1] === 'number',
    ),
  );
  const parsed = layout.selectedPorts
    ? externalProcessorNode(base, descriptor, layout.selectedPorts, presentation)
    : unavailableProcessorNode(node, base, descriptor, runtime, presentation);
  parsed.appearance.title = node.label ?? descriptor.displayName;
  parsed.appearance.parameterControls = externalParameterControls(descriptor, values);
  if (layout.unavailable) {
    parsed.appearance.offline = true;
    parsed.appearance.detail = presentation.processorLayoutUnavailable;
  }
  parsed.runtime = layout.materializeSelectedPorts
    ? { ...runtime, config: { ...config, ports: layout.selectedPorts } }
    : runtime;
  return parsed;
}

function resolveProcessorLayout(
  descriptor: ExternalProcessorDescriptor,
  authoredPorts: ProviderPorts | null,
  hasAuthoredPorts: boolean,
) {
  const defaultLayout = providerPortsFrom(
    descriptor.layouts.find((layout) => layout.layoutId === descriptor.defaultLayoutId),
  );
  const selectedPorts = hasAuthoredPorts ? authoredPorts : defaultLayout;
  const catalogLayout = selectedPorts
    ? providerPortsFrom(
        descriptor.layouts.find((layout) => layout.layoutId === selectedPorts.layoutId),
      )
    : null;
  return {
    materializeSelectedPorts: !hasAuthoredPorts && selectedPorts !== null,
    selectedPorts,
    unavailable:
      !selectedPorts || !catalogLayout || !providerPortsMatch(selectedPorts, catalogLayout),
  };
}

function unavailableProcessorNode(
  node: RuntimeGraphNode,
  base: { id: string; position: AudioGraphPosition },
  descriptor: ExternalProcessorDescriptor,
  runtime: NonNullable<AudioGraphNode['runtime']>,
  presentation: GraphPresentation,
): AudioGraphNode {
  return {
    ...base,
    appearance: {
      category: `${presentation.processor} · ${descriptor.format.toUpperCase()}`,
      detail: presentation.processorLayoutUnavailable,
      hasProcessorEditor: descriptor.hasEditor,
      icon: descriptor.format.toUpperCase(),
      offline: true,
      title: node.label ?? descriptor.displayName,
    },
    inputPorts: [],
    outputPorts: [],
    runtime,
  };
}

function runtimeDynamicsNode(
  node: RuntimeGraphNode,
  base: { id: string; position: AudioGraphPosition },
  runtime: NonNullable<AudioGraphNode['runtime']>,
  presentation: GraphPresentation,
): AudioGraphNode | null {
  if (node.type !== 'compressor' && node.type !== 'limiter') return null;
  const config = objectValue(node.config) ?? {};
  const compressor = node.type === 'compressor';
  const parsed = processorNode(
    base,
    node.type,
    node.label ?? (compressor ? presentation.compressor : presentation.limiter),
    compressor ? 'COMP' : 'LIM',
    config,
    compressor ? compressorControls(config, presentation) : limiterControls(config, presentation),
    presentation,
  );
  parsed.runtime = runtime;
  return parsed;
}

function unsupportedRuntimeNode(
  node: RuntimeGraphNode,
  base: { id: string; position: AudioGraphPosition },
  presentation: GraphPresentation,
): AudioGraphNode {
  return {
    ...base,
    appearance: {
      category: presentation.unsupported,
      detail: node.type,
      icon: '?',
      title: node.label ?? node.id,
    },
    inputPorts: [],
    outputPorts: [],
    runtime: { config: node.config, type: node.type, label: node.label },
  };
}

function runtimeNodePosition(type: string, index: number): AudioGraphPosition {
  const y = 96 + index * 156;
  if (type === 'application_capture' || type === 'audio_device_input') return { x: 48, y };
  if (type === 'audio_device_output') return { x: 728, y };
  return { x: 388, y };
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : null;
}

function directNumber(value: unknown, key: string): number | null {
  const candidate = objectValue(value)?.[key];
  return typeof candidate === 'number' ? candidate : null;
}

function directString(value: unknown, key: string): string | null {
  const candidate = objectValue(value)?.[key];
  return typeof candidate === 'string' ? candidate : null;
}

export function virtualEndpointIdentity(node: AudioGraphNode): string | null {
  return node.runtime?.type === 'virtual_endpoint' ? directString(node.runtime.config, 'id') : null;
}

function nestedNumber(value: unknown, parent: string, key: string): number | null {
  return directNumber(objectValue(value)?.[parent], key);
}

function nestedString(value: unknown, parent: string, key: string): string | null {
  const candidate = objectValue(objectValue(value)?.[parent])?.[key];
  return typeof candidate === 'string' ? candidate : null;
}

function directStringArray(value: unknown, key: string): string[] {
  const candidate = objectValue(value)?.[key];
  return Array.isArray(candidate)
    ? candidate.filter((item): item is string => typeof item === 'string')
    : [];
}

function virtualEndpointNodes(
  node: RuntimeGraphNode,
  presentation: GraphPresentation,
  reservedIds: ReadonlySet<string>,
): AudioGraphNode[] {
  const title = directString(node.config, 'displayName') ?? node.label ?? node.id;
  const renderChannels = nestedNumber(node.config, 'render', 'channels') ?? 0;
  const captureChannels = nestedNumber(node.config, 'capture', 'channels') ?? 0;
  const reference = objectValue(objectValue(node.config)?.reference);
  if (!reference) return [];
  const control = endpointControl(title, renderChannels, captureChannels, presentation);
  const runtime = { config: node.config, type: node.type };
  const nodes: AudioGraphNode[] = [];
  if (reference.render === true && captureChannels > 0) {
    nodes.push({
      appearance: {
        category: presentation.virtualInput,
        gainControl: {
          label: presentation.sourceGain,
          valueDb: directNumber(node.config, 'gainDb') ?? 0,
        },
        channelCount: captureChannels,
        endpointControl: control,
        icon: 'IN',
        meterBanks: [{ channels: captureChannels, nodeId: node.id, portId: FROM_OS_PORT_ID }],
        optionsLabel: presentation.options,
        title,
      },
      id: virtualViewId(node.id, 'source', reservedIds),
      inputPorts: [],
      outputPorts: channelPorts(
        FROM_OS_PORT_ID,
        captureChannels,
        presentation.virtualSourcePort,
        'source',
      ),
      position: { x: 48, y: 96 },
      runtime: { ...runtime, binding: { nodeId: node.id, side: 'source' } },
    });
  }
  if (reference.capture === true && renderChannels > 0) {
    nodes.push({
      appearance: {
        category: presentation.virtualOutput,
        channelCount: renderChannels,
        endpointControl: control,
        icon: 'OUT',
        meterBanks: [{ channels: renderChannels, nodeId: node.id, portId: TO_OS_PORT_ID }],
        optionsLabel: presentation.options,
        title,
      },
      id: virtualViewId(node.id, 'sink', reservedIds),
      inputPorts: channelPorts(TO_OS_PORT_ID, renderChannels, presentation.virtualSinkPort, 'sink'),
      outputPorts: [],
      position: { x: 728, y: 96 },
      runtime: { ...runtime, binding: { nodeId: node.id, side: 'sink' } },
    });
  }
  return nodes;
}

function endpointControl(
  displayName: string,
  renderChannels: number,
  captureChannels: number,
  presentation: GraphPresentation,
): NonNullable<AudioGraphNode['appearance']['endpointControl']> {
  return {
    captureChannels,
    captureChannelsLabel: presentation.captureChannels,
    displayName,
    nameLabel: presentation.deviceName,
    renderChannels,
    renderChannelsLabel: presentation.renderChannels,
  };
}

function permitsEndpointEdit(
  settings: VirtualEndpointSettings | undefined,
  render: number,
  capture: number,
  unchangedName: boolean,
) {
  return (
    !settings ||
    ((render === 0 || settings.renderChannels.includes(render)) &&
      (capture === 0 || settings.captureChannels.includes(capture)) &&
      (settings.canRename || unchangedName))
  );
}

function updatedVirtualDeviceConfig(
  config: Record<string, unknown>,
  update: VirtualEndpointUpdate,
) {
  return {
    ...config,
    ...(config.capture ? { capture: streamConfig(config.capture, update.captureChannels) } : {}),
    ...(config.render ? { render: streamConfig(config.render, update.renderChannels) } : {}),
    displayName: update.displayName,
  };
}

function editedEndpointChannels(config: Record<string, unknown>, update: VirtualEndpointUpdate) {
  const renderChannels = config.render
    ? supportedEndpointChannels(update.renderChannels)
    : update.renderChannels === 0
      ? 0
      : null;
  const captureChannels = config.capture
    ? supportedEndpointChannels(update.captureChannels)
    : update.captureChannels === 0
      ? 0
      : null;
  if (renderChannels === null || captureChannels === null) return null;
  return { renderChannels, captureChannels };
}

function supportedEndpointChannels(value: number): number | null {
  // Validate geometry here; the advertised backend policy above owns the
  // actual supported set. A stale preset list must not reject a visible option.
  return Number.isInteger(value) && value >= 1 && value <= 64 ? value : null;
}

function streamConfig(value: unknown, channels: number): RuntimeStreamSpec {
  return {
    ...(objectValue(value) ?? {}),
    channelLayout: channels === 1 ? 'mono' : channels === 2 ? 'stereo' : 'discrete',
    channels,
  };
}

function gainValue(config: unknown): number {
  if (typeof config === 'object' && config !== null && 'gainDb' in config) {
    const value = (config as { gainDb: unknown }).gainDb;
    if (typeof value === 'number') return value;
  }
  return 0;
}

function runtimeEdge(
  model: AudioGraphModel,
  edge: AudioGraphModel['edges'][number],
): RuntimeGraphEdge {
  const source = channelBinding(model, edge.sourceNodeId, edge.sourcePortId, 'outputPorts');
  const destination = channelBinding(model, edge.targetNodeId, edge.targetPortId, 'inputPorts');
  return {
    channelMapping: {
      mode: 'explicit',
      routes: [
        {
          destinationChannel: destination.channelIndex,
          sourceChannel: source.channelIndex,
        },
      ],
    },
    destination: {
      nodeId: destination.nodeId,
      portId: destination.runtimePortId,
    },
    id: edge.id,
    source: { nodeId: source.nodeId, portId: source.runtimePortId },
  };
}

function runtimeEdgeToAudioEdges(
  edge: RuntimeGraphEdge,
  nodes: readonly AudioGraphNode[],
): AudioGraphModel['edges'] {
  const sourceNodeId = graphNodeId(nodes, edge.source.nodeId, edge.source.portId);
  const targetNodeId = graphNodeId(nodes, edge.destination.nodeId, edge.destination.portId);
  const routes = runtimeRoutes(edge, nodes, sourceNodeId, targetNodeId);
  return routes.flatMap((route, routeIndex) => {
    const sourcePortId = channelPortFor(
      nodes,
      sourceNodeId,
      edge.source.portId,
      route.sourceChannel,
      'outputPorts',
    );
    const targetPortId = channelPortFor(
      nodes,
      targetNodeId,
      edge.destination.portId,
      route.destinationChannel,
      'inputPorts',
    );
    if (!sourcePortId || !targetPortId) return [];
    return [
      {
        id: `${edge.id}${CHANNEL_PORT_SEPARATOR}${routeIndex}`,
        sourceNodeId,
        sourcePortId,
        targetNodeId,
        targetPortId,
      },
    ];
  });
}

function runtimeRoutes(
  edge: RuntimeGraphEdge,
  nodes: readonly AudioGraphNode[],
  sourceNodeId: string,
  targetNodeId: string,
): RuntimeChannelRoute[] {
  if (edge.channelMapping.mode === 'explicit') return edge.channelMapping.routes;
  const sourceChannels = portChannelCount(nodes, sourceNodeId, edge.source.portId, 'outputPorts');
  const targetChannels = portChannelCount(
    nodes,
    targetNodeId,
    edge.destination.portId,
    'inputPorts',
  );
  return Array.from({ length: Math.min(sourceChannels, targetChannels) }, (_, channel) => ({
    destinationChannel: channel,
    sourceChannel: channel,
  }));
}

function channelPorts(
  runtimePortId: string,
  channels: number,
  label: string,
  boundary?: 'sink' | 'source',
): AudioGraphNode['inputPorts'] {
  const count = Math.max(0, Math.min(64, Math.trunc(channels)));
  return Array.from({ length: count }, (_, channelIndex) => ({
    boundary,
    channelIndex,
    id: `${runtimePortId}${CHANNEL_PORT_SEPARATOR}${channelIndex}`,
    label: `${label} ${channelIndex + 1}`,
    runtimePortId,
  }));
}

function channelBinding(
  model: AudioGraphModel,
  nodeId: string,
  visualPortId: string,
  direction: 'inputPorts' | 'outputPorts',
): { nodeId: string; channelIndex: number; runtimePortId: string } {
  const node = model.nodes.find((candidate) => candidate.id === nodeId);
  const port = node?.[direction].find((candidate) => candidate.id === visualPortId);
  return {
    nodeId: node ? runtimeNodeId(node) : nodeId,
    channelIndex: port?.channelIndex ?? 0,
    runtimePortId: port?.runtimePortId ?? visualPortId,
  };
}

function channelPortFor(
  nodes: readonly AudioGraphNode[],
  nodeId: string,
  runtimePortId: string,
  channelIndex: number,
  direction: 'inputPorts' | 'outputPorts',
): string | null {
  const port = nodes
    .find((node) => node.id === nodeId)
    ?.[direction].find(
      (candidate) =>
        (candidate.runtimePortId ?? candidate.id) === runtimePortId &&
        (candidate.channelIndex ?? 0) === channelIndex,
    );
  return port?.id ?? null;
}

function portChannelCount(
  nodes: readonly AudioGraphNode[],
  nodeId: string,
  runtimePortId: string,
  direction: 'inputPorts' | 'outputPorts',
): number {
  return (
    nodes
      .find((node) => node.id === nodeId)
      ?.[direction].filter((port) => (port.runtimePortId ?? port.id) === runtimePortId).length ?? 0
  );
}

function graphNodeId(nodes: readonly AudioGraphNode[], nodeId: string, portId: string): string {
  const side = portId === TO_OS_PORT_ID ? 'sink' : 'source';
  return (
    nodes.find(
      (node) => node.runtime?.binding?.nodeId === nodeId && node.runtime.binding.side === side,
    )?.id ?? nodeId
  );
}

function runtimeNodeId(node: AudioGraphNode): string {
  return node.runtime?.binding?.nodeId ?? node.id;
}

function virtualViewId(
  nodeId: string,
  side: 'source' | 'sink',
  reservedIds: ReadonlySet<string>,
): string {
  let id = `${nodeId}${side === 'source' ? SOURCE_NODE_SUFFIX : SINK_NODE_SUFFIX}`;
  if (!reservedIds.has(id)) return id;
  // Escaped IDs end in `]`, unlike ordinary virtual view IDs. JSON keeps each
  // node/side pair distinct; leading # characters only avoid actual node IDs.
  id = `@virtual:${JSON.stringify([nodeId, side])}`;
  while (reservedIds.has(id)) id = `#${id}`;
  return id;
}
