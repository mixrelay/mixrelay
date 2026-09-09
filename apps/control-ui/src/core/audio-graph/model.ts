import type { RuntimeGraphEdge } from '../control-port/types';
import type { EndpointControl } from '../ui/AudioNodeCard';
import type { StatusTone } from '../ui/StatusBadge';

export interface AudioGraphPosition {
  x: number;
  y: number;
}

export interface AudioGraphPort {
  boundary?: 'sink' | 'source';
  channelIndex?: number;
  id: string;
  label: string;
  runtimePortId?: string;
}

export interface AudioGraphNodeAppearance {
  category: string;
  channelCount?: number;
  detail?: string;
  endpointControl?: EndpointControl;
  gainControl?: { label: string; valueDb: number };
  hasProcessorEditor?: boolean;
  icon: string;
  iconUrl?: string;
  meterChannels?: number;
  meterSource?: { nodeId: string; portId: string };
  optionsLabel?: string;
  parameterControls?: readonly AudioNodeParameterControl[];
  status?: string;
  statusTone?: StatusTone;
  title: string;
}

export interface AudioNodeParameterControl {
  id: string;
  label: string;
  maximum: number;
  minimum: number;
  step: number;
  suffix: string;
  value: number;
}

export interface AudioGraphNode {
  appearance: AudioGraphNodeAppearance;
  id: string;
  inputPorts: AudioGraphPort[];
  outputPorts: AudioGraphPort[];
  position: AudioGraphPosition;
  runtime?: {
    /** Unresolved presentation retains the original nullable label, not its fallback title. */
    label?: string | null;
    binding?: { nodeId: string; side: 'source' | 'sink' };
    config: unknown;
    type: string;
  };
}

export interface AudioGraphEdge {
  id: string;
  sourceNodeId: string;
  sourcePortId: string;
  targetNodeId: string;
  targetPortId: string;
}

export interface AudioGraphModel {
  /** Native topology plus its view projection; unedited groups keep their exact IDs/mapping. */
  edgeProjections?: { edge: RuntimeGraphEdge; views: AudioGraphEdge[] }[];
  edges: AudioGraphEdge[];
  nodes: AudioGraphNode[];
  revision: number;
}

export interface AudioGraphConnection {
  sourceNodeId: string;
  sourcePortId: string;
  targetNodeId: string;
  targetPortId: string;
}

function hasValidPorts(ports: AudioGraphPort[]): boolean {
  const ids = ports.map((port) => port.id);
  return ids.every((id) => id.length > 0) && new Set(ids).size === ids.length;
}

function isValidNode(node: AudioGraphNode): boolean {
  return node.id.length > 0 && hasValidPorts(node.inputPorts) && hasValidPorts(node.outputPorts);
}

function hasPort(ports: AudioGraphPort[], portId: string): boolean {
  return ports.some((port) => port.id === portId);
}

function createsCycle(
  model: AudioGraphModel,
  connection: AudioGraphConnection,
  ignoredEdgeId?: string,
): boolean {
  const destinations = new Map<string, string[]>();
  for (const edge of model.edges) {
    if (edge.id === ignoredEdgeId) continue;
    if (!ordersProcessing(model, edge)) continue;
    const targets = destinations.get(edge.sourceNodeId) ?? [];
    targets.push(edge.targetNodeId);
    destinations.set(edge.sourceNodeId, targets);
  }
  const pending = [connection.targetNodeId];
  const visited = new Set<string>();
  while (pending.length > 0) {
    const nodeId = pending.pop() as string;
    if (visited.has(nodeId)) continue;
    if (nodeId === connection.sourceNodeId) return true;
    visited.add(nodeId);
    pending.push(...(destinations.get(nodeId) ?? []));
  }
  return false;
}

function portBoundary(
  model: AudioGraphModel,
  nodeId: string,
  portId: string,
  direction: 'inputPorts' | 'outputPorts',
) {
  return model.nodes
    .find((node) => node.id === nodeId)
    ?.[direction].find((port) => port.id === portId)?.boundary;
}

function ordersProcessing(model: AudioGraphModel, connection: AudioGraphConnection): boolean {
  return (
    portBoundary(model, connection.sourceNodeId, connection.sourcePortId, 'outputPorts') !==
      'source' &&
    portBoundary(model, connection.targetNodeId, connection.targetPortId, 'inputPorts') !== 'sink'
  );
}

export function canConnectAudioGraph(
  model: AudioGraphModel,
  connection: AudioGraphConnection,
  ignoredEdgeId?: string,
): boolean {
  if (
    (connection.sourceNodeId === connection.targetNodeId && ordersProcessing(model, connection)) ||
    connection.sourcePortId.length === 0 ||
    connection.targetPortId.length === 0
  ) {
    return false;
  }
  const source = model.nodes.find((node) => node.id === connection.sourceNodeId);
  const target = model.nodes.find((node) => node.id === connection.targetNodeId);
  if (
    !source ||
    !target ||
    !hasPort(source.outputPorts, connection.sourcePortId) ||
    !hasPort(target.inputPorts, connection.targetPortId)
  ) {
    return false;
  }
  const conflicts = model.edges.some(
    (edge) =>
      edge.id !== ignoredEdgeId &&
      edge.sourceNodeId === connection.sourceNodeId &&
      edge.sourcePortId === connection.sourcePortId &&
      edge.targetNodeId === connection.targetNodeId &&
      edge.targetPortId === connection.targetPortId,
  );
  return (
    !conflicts &&
    (!ordersProcessing(model, connection) || !createsCycle(model, connection, ignoredEdgeId))
  );
}

function connectionId(connection: AudioGraphConnection): string {
  return [
    'edge',
    connection.sourceNodeId,
    connection.sourcePortId,
    connection.targetNodeId,
    connection.targetPortId,
  ]
    .map(encodeURIComponent)
    .join(':');
}

export function connectAudioGraph(
  model: AudioGraphModel,
  connection: AudioGraphConnection,
): AudioGraphModel {
  return connectAudioGraphBatch(model, [connection]);
}

export function connectAudioGraphBatch(
  model: AudioGraphModel,
  connections: readonly AudioGraphConnection[],
): AudioGraphModel {
  if (connections.length === 0) return model;
  const edges = [...model.edges];
  const pendingModel = { ...model, edges };
  for (const connection of connections) {
    if (!canConnectAudioGraph(pendingModel, connection)) continue;
    edges.push({ id: connectionId(connection), ...connection });
  }
  if (edges.length === model.edges.length) return model;
  return {
    ...model,
    edges,
    revision: model.revision + 1,
  };
}

export function addAudioGraphNode(model: AudioGraphModel, node: AudioGraphNode): AudioGraphModel {
  if (!isValidNode(node) || model.nodes.some((existing) => existing.id === node.id)) return model;
  return { ...model, nodes: [...model.nodes, node], revision: model.revision + 1 };
}

export function reconnectAudioGraph(
  model: AudioGraphModel,
  edgeId: string,
  connection: AudioGraphConnection,
): AudioGraphModel {
  const edgeIndex = model.edges.findIndex((edge) => edge.id === edgeId);
  if (edgeIndex < 0 || !canConnectAudioGraph(model, connection, edgeId)) return model;
  return {
    ...model,
    edges: model.edges.map((edge, index) =>
      index === edgeIndex ? { id: edgeId, ...connection } : edge,
    ),
    revision: model.revision + 1,
  };
}

export function disconnectAudioGraph(
  model: AudioGraphModel,
  edgeIds: readonly string[],
): AudioGraphModel {
  const removedIds = new Set(edgeIds);
  const edges = model.edges.filter((edge) => !removedIds.has(edge.id));
  return edges.length === model.edges.length
    ? model
    : { ...model, edges, revision: model.revision + 1 };
}

export function removeAudioGraphItems(
  model: AudioGraphModel,
  nodeIds: readonly string[],
  edgeIds: readonly string[] = [],
): AudioGraphModel {
  const removedNodeIds = new Set(nodeIds);
  const removedEdgeIds = new Set(edgeIds);
  const nodes =
    removedNodeIds.size === 0
      ? model.nodes
      : model.nodes.filter((node) => !removedNodeIds.has(node.id));
  const edges =
    removedNodeIds.size === 0 && removedEdgeIds.size === 0
      ? model.edges
      : model.edges.filter(
          (edge) =>
            !removedEdgeIds.has(edge.id) &&
            !removedNodeIds.has(edge.sourceNodeId) &&
            !removedNodeIds.has(edge.targetNodeId),
        );
  return nodes.length === model.nodes.length && edges.length === model.edges.length
    ? model
    : { ...model, edges, nodes, revision: model.revision + 1 };
}

export function moveAudioGraphNode(
  model: AudioGraphModel,
  nodeId: string,
  position: AudioGraphPosition,
): AudioGraphModel {
  const nodeIndex = model.nodes.findIndex((node) => node.id === nodeId);
  const node = model.nodes[nodeIndex];
  if (!node || (node.position.x === position.x && node.position.y === position.y)) return model;
  return {
    ...model,
    nodes: model.nodes.map((node, index) => (index === nodeIndex ? { ...node, position } : node)),
  };
}
