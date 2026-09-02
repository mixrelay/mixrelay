export type NodeGroup = 'source' | 'processor' | 'analysis' | 'output';
export type CapabilityState = 'foundation' | 'planned' | 'provider';

export interface NodeCapability {
  descriptionKey: string;
  group: NodeGroup;
  icon: string;
  id: string;
  nameKey: string;
  state: CapabilityState;
}

export interface GraphNode {
  capabilityId: string;
  detailKey: string;
  id: string;
  inputCount: number;
  outputCount: number;
  placement: string;
}

export interface GraphConnection {
  id: string;
  path: string;
  tone: 'mint' | 'amber';
}

export const nodeCatalog: readonly NodeCapability[] = [
  capability('application', 'source', 'APP', 'foundation'),
  capability('systemAudio', 'source', 'SYS', 'planned'),
  capability('inputDevice', 'source', 'IN', 'foundation'),
  capability('passThru', 'source', '↳', 'planned'),
  capability('audioFile', 'source', 'WAV', 'planned'),
  capability('browserRemote', 'source', 'LAN', 'planned'),
  capability('mixer', 'processor', 'Σ', 'foundation'),
  capability('gain', 'processor', 'dB', 'foundation'),
  capability('compressor', 'processor', 'CMP', 'planned'),
  capability('parametricEq', 'processor', 'EQ', 'planned'),
  capability('ducking', 'processor', 'DUK', 'planned'),
  capability('speechDenoise', 'processor', 'NR', 'provider'),
  capability('delaySync', 'processor', 'ms', 'planned'),
  capability('audioUnit', 'processor', 'AU', 'provider'),
  capability('levelMeter', 'analysis', 'VU', 'foundation'),
  capability('spectrum', 'analysis', 'FFT', 'planned'),
  capability('loudness', 'analysis', 'LU', 'planned'),
  capability('virtualDevice', 'output', 'OBS', 'foundation'),
  capability('monitor', 'output', 'OUT', 'foundation'),
  capability('recorder', 'output', 'REC', 'planned'),
  capability('rtmp', 'output', 'LIVE', 'planned'),
  capability('icecast', 'output', 'CAST', 'planned'),
];

export const broadcastGraph: readonly GraphNode[] = [
  graphNode('chrome', 'application', 'chrome', 0, 1, 'chrome'),
  graphNode('microphone', 'inputDevice', 'microphone', 0, 1, 'microphone'),
  graphNode('mixer', 'mixer', 'twoSources', 2, 1, 'mixer'),
  graphNode('compressor', 'compressor', 'streamControl', 1, 1, 'compressor'),
  graphNode('equalizer', 'parametricEq', 'voiceContour', 1, 1, 'equalizer'),
  graphNode('obs', 'virtualDevice', 'stereo48k', 1, 0, 'obs'),
  graphNode('monitor', 'monitor', 'headphones', 1, 0, 'monitor'),
];

export const broadcastConnections: readonly GraphConnection[] = [
  { id: 'chrome-mixer', path: 'M 220 168 C 280 168, 255 273, 335 273', tone: 'mint' },
  { id: 'mic-mixer', path: 'M 220 410 C 285 410, 260 303, 335 303', tone: 'amber' },
  { id: 'mixer-compressor', path: 'M 535 288 C 575 288, 565 228, 610 228', tone: 'mint' },
  { id: 'compressor-eq', path: 'M 810 228 C 845 228, 825 348, 860 348', tone: 'mint' },
  { id: 'eq-obs', path: 'M 1060 348 C 1110 348, 1090 180, 1140 180', tone: 'mint' },
  { id: 'eq-monitor', path: 'M 1060 348 C 1110 348, 1090 440, 1140 440', tone: 'amber' },
];

export function findCapability(id: string) {
  const match = nodeCatalog.find((capability) => capability.id === id);
  if (!match) throw new Error(`Unknown node capability: ${id}`);
  return match;
}

export function findGraphNode(id: string) {
  const match = broadcastGraph.find((node) => node.id === id);
  if (!match) throw new Error(`Unknown graph node: ${id}`);
  return match;
}

function capability(
  id: string,
  group: NodeGroup,
  icon: string,
  state: CapabilityState,
): NodeCapability {
  return {
    descriptionKey: `studioGraph.nodes.${id}.description`,
    group,
    icon,
    id,
    nameKey: `studioGraph.nodes.${id}.name`,
    state,
  };
}

function graphNode(
  id: string,
  capabilityId: string,
  detailKey: string,
  inputCount: number,
  outputCount: number,
  placement: string,
): GraphNode {
  return { capabilityId, detailKey, id, inputCount, outputCount, placement };
}
