import type { Edge, Node } from '@xyflow/react';
import { canvasLayers } from '../theme/tokens';

import {
  type AudioGraphEdge,
  type AudioGraphModel,
  type AudioGraphNode,
  type AudioGraphPort,
  audioGraphNodeTitle,
  normalizeAudioGraphNodeSize,
} from './model';

export interface AudioFlowNodeData extends Record<string, unknown> {
  graphNode: AudioGraphNode;
}

interface AudioFlowEdgeData extends Record<string, unknown> {
  graphEdge: AudioGraphEdge;
}

export type AudioFlowNode = Node<AudioFlowNodeData, 'audio'>;
export type AudioFlowEdge = Edge<AudioFlowEdgeData>;

/** Open disclosures stay above selected nodes and below portalled menus. */
export function applyAudioNodeLayers(nodes: AudioFlowNode[], openedNodeId: string | null) {
  return nodes.map((node) => {
    const zIndex =
      node.id === openedNodeId
        ? canvasLayers.nodeOptionsOpen
        : node.selected
          ? canvasLayers.nodeSelected
          : canvasLayers.node;
    return node.zIndex === zIndex ? node : { ...node, zIndex };
  });
}

function createFlowNode(graphNode: AudioGraphNode, previous?: AudioFlowNode): AudioFlowNode {
  return {
    ...previous,
    data: { graphNode },
    deletable: true,
    ariaLabel: audioGraphNodeTitle(graphNode),
    id: graphNode.id,
    position: graphNode.position,
    width: normalizeAudioGraphNodeSize(graphNode, graphNode.size)?.width,
    // The card owns its collapsed size and temporary disclosure height. Let
    // React Flow measure that rendered height instead of fixing the outer box.
    height: undefined,
    type: 'audio',
    zIndex: canvasLayers.node,
  };
}

/** Named cable endpoints; channel numbers match the port handles. A port name is
 * present only when another port on the same side shares that channel number. */
export interface AudioGraphEdgeEndpoints {
  source: string;
  sourceChannel: number;
  sourcePort?: string;
  target: string;
  targetChannel: number;
  targetPort?: string;
}

export type AudioFlowEdgeLabel = (endpoints: AudioGraphEdgeEndpoints) => string;

/** Accessible port names on one side with their handle channel numbers (an explicit
 * channel index, otherwise the port position). Ports that share both a label and a
 * channel number (e.g. two buses named Main) get an ordinal in port-ID order, skipping
 * names already used on that channel. Names survive reordering while each port's channel
 * stays the same, which explicit channelIndex values guarantee. */
export function namedAudioPorts(ports: readonly AudioGraphPort[]) {
  const entries = ports.map((port, index) => ({ port, channel: (port.channelIndex ?? index) + 1 }));
  const names = new Map(entries.map(({ port }) => [port.id, port.label]));
  for (const channel of new Set(entries.map((entry) => entry.channel))) {
    const group = entries.filter((entry) => entry.channel === channel).map(({ port }) => port);
    const count = (label: string) => group.filter((port) => port.label === label).length;
    const taken = new Set(
      group.filter((port) => count(port.label) === 1).map((port) => port.label),
    );
    const repeated = group.filter((port) => count(port.label) > 1);
    for (const port of [...repeated].sort((a, b) => (a.id < b.id ? -1 : 1))) {
      let ordinal = 1;
      while (taken.has(`${port.label} (${ordinal})`)) ordinal += 1;
      const name = `${port.label} (${ordinal})`;
      taken.add(name);
      names.set(port.id, name);
    }
  }
  return entries.map((entry) => ({ ...entry, name: names.get(entry.port.id) as string }));
}

function portEndpoint(ports: readonly AudioGraphPort[], portId: string) {
  const named = namedAudioPorts(ports);
  const entry = named.find(({ port }) => port.id === portId);
  if (!entry) return null;
  const shared = named.filter(({ channel }) => channel === entry.channel).length > 1;
  return { channel: entry.channel, port: shared ? entry.name : undefined };
}

export function audioGraphEdgeEndpoints(
  nodesById: ReadonlyMap<string, AudioGraphNode>,
  graphEdge: AudioGraphEdge,
): AudioGraphEdgeEndpoints | null {
  const source = nodesById.get(graphEdge.sourceNodeId);
  const target = nodesById.get(graphEdge.targetNodeId);
  if (!source || !target) return null;
  const from = portEndpoint(source.outputPorts, graphEdge.sourcePortId);
  const to = portEndpoint(target.inputPorts, graphEdge.targetPortId);
  if (!from || !to) return null;
  return {
    source: audioGraphNodeTitle(source),
    sourceChannel: from.channel,
    sourcePort: from.port,
    target: audioGraphNodeTitle(target),
    targetChannel: to.channel,
    targetPort: to.port,
  };
}

function createFlowEdge(
  graphEdge: AudioGraphEdge,
  previous: AudioFlowEdge | undefined,
  ariaLabel: string | undefined,
): AudioFlowEdge {
  return {
    ...previous,
    ariaLabel,
    data: { graphEdge },
    id: graphEdge.id,
    reconnectable: true,
    source: graphEdge.sourceNodeId,
    sourceHandle: graphEdge.sourcePortId,
    target: graphEdge.targetNodeId,
    targetHandle: graphEdge.targetPortId,
    type: 'default',
    zIndex: canvasLayers.cable,
  };
}

export function reconcileAudioFlowNodes(
  current: AudioFlowNode[],
  model: AudioGraphModel,
): AudioFlowNode[] {
  const currentById = new Map(current.map((node) => [node.id, node]));
  const next = model.nodes.map((graphNode) => {
    const previous = currentById.get(graphNode.id);
    return previous?.data.graphNode === graphNode ? previous : createFlowNode(graphNode, previous);
  });
  return next.length === current.length && next.every((node, index) => node === current[index])
    ? current
    : next;
}

export function reconcileAudioFlowEdges(
  current: AudioFlowEdge[],
  model: AudioGraphModel,
  edgeLabel?: AudioFlowEdgeLabel,
): AudioFlowEdge[] {
  const currentById = new Map(current.map((edge) => [edge.id, edge]));
  const nodesById = new Map(edgeLabel ? model.nodes.map((node) => [node.id, node]) : []);
  const next = model.edges.map((graphEdge) => {
    const previous = currentById.get(graphEdge.id);
    const endpoints = edgeLabel ? audioGraphEdgeEndpoints(nodesById, graphEdge) : null;
    const ariaLabel = endpoints && edgeLabel ? edgeLabel(endpoints) : undefined;
    return previous?.data?.graphEdge === graphEdge && previous.ariaLabel === ariaLabel
      ? previous
      : createFlowEdge(graphEdge, previous, ariaLabel);
  });
  return next.length === current.length && next.every((edge, index) => edge === current[index])
    ? current
    : next;
}
