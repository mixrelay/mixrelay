import { type Dispatch, type SetStateAction, useState } from 'react';
import type { HierarchicalMenuItem } from '../ui/HierarchicalMenu';
import { type AudioGraphModel, disconnectAudioGraph, removeAudioGraphItems } from './model';
import type { AudioFlowEdge, AudioFlowNode } from './reactFlowAdapter';

export interface GraphContextMenuLabels {
  renameNode?: string;
  removeNode: string;
  removeSelection: string;
  disconnect: string;
  disconnectPort: string;
  disconnectPorts: string;
  removeCable: string;
  fitView: string;
  selectAll: string;
  clearSelection: string;
}

const defaultLabels: GraphContextMenuLabels = {
  removeNode: 'Delete node',
  removeSelection: 'Delete selection',
  disconnect: 'Disconnect cables',
  disconnectPort: 'Disconnect channel',
  disconnectPorts: 'Disconnect selected channels',
  removeCable: 'Delete cable',
  fitView: 'Fit graph',
  selectAll: 'Select all',
  clearSelection: 'Clear selection',
};

interface PortTarget {
  nodeId: string;
  portId: string;
  side: 'input' | 'output';
}

type Scope = { nodes: string[]; edges: string[] } & (
  | { kind: 'canvas' | 'node' | 'edge' | 'selection' }
  | { kind: 'port'; ports: readonly PortTarget[] }
);

interface ContextOptions {
  model: AudioGraphModel;
  nodes: AudioFlowNode[];
  edges: AudioFlowEdge[];
  setNodes: Dispatch<SetStateAction<AudioFlowNode[]>>;
  setEdges: Dispatch<SetStateAction<AudioFlowEdge[]>>;
  addItems: readonly HierarchicalMenuItem[];
  labels?: GraphContextMenuLabels;
  renameNode?: (nodeId: string) => void;
  commit: (model: AudioGraphModel, kind: 'runtime') => void;
  fitView: () => void;
  clearPorts: () => void;
  selectPorts: (ports: readonly PortTarget[]) => void;
  selectedPorts: { current: readonly PortTarget[] };
}

function resolveScope(target: Element, nodes: AudioFlowNode[], edges: AudioFlowEdge[]): Scope {
  const selected = {
    nodes: nodes.filter((n) => n.selected).map((n) => n.id),
    edges: edges.filter((e) => e.selected).map((e) => e.id),
  };
  const port = target.closest<HTMLElement>('[data-audio-port-id]')?.dataset;
  if (
    port?.audioNodeId &&
    port.audioPortId &&
    (port.audioPortSide === 'input' || port.audioPortSide === 'output')
  ) {
    return {
      kind: 'port',
      nodes: [],
      edges: [],
      ports: [
        {
          nodeId: port.audioNodeId,
          portId: port.audioPortId,
          side: port.audioPortSide,
        },
      ],
    };
  }
  if (target.closest('.react-flow__nodesselection-rect')) {
    return { kind: 'selection', ...selected };
  }
  const nodeId = target.closest('.react-flow__node')?.getAttribute('data-id');
  if (nodeId)
    return {
      kind: 'node',
      ...(selected.nodes.includes(nodeId) ? selected : { nodes: [nodeId], edges: [] }),
    };
  const edgeId = target.closest('.react-flow__edge')?.getAttribute('data-id');
  if (edgeId)
    return {
      kind: 'edge',
      ...(selected.edges.includes(edgeId) ? selected : { nodes: [], edges: [edgeId] }),
    };
  return { kind: 'canvas', ...selected };
}

function connectedEdges(model: AudioGraphModel, scope: Scope): string[] {
  const nodes = new Set(scope.nodes);
  return model.edges
    .filter((edge) => {
      if (scope.kind === 'port') {
        return scope.ports.some(({ nodeId, portId, side }) =>
          side === 'input'
            ? edge.targetNodeId === nodeId && edge.targetPortId === portId
            : edge.sourceNodeId === nodeId && edge.sourcePortId === portId,
        );
      }
      return (
        nodes.has(edge.sourceNodeId) ||
        nodes.has(edge.targetNodeId) ||
        scope.edges.includes(edge.id)
      );
    })
    .map((edge) => edge.id);
}

function renameItems(scope: Scope, options: ContextOptions): HierarchicalMenuItem[] {
  if (scope.kind !== 'node' || scope.nodes.length !== 1 || !options.renameNode) return [];
  return [
    {
      id: 'context:rename',
      label: options.labels?.renameNode ?? 'Rename display name',
      description: 'F2',
    },
  ];
}

function contextItems(scope: Scope, options: ContextOptions): HierarchicalMenuItem[] {
  const labels = options.labels ?? defaultLabels;
  const count = scope.nodes.length + scope.edges.length;
  if (scope.kind === 'port')
    return [
      {
        id: 'context:disconnect',
        label: scope.ports.length > 1 ? labels.disconnectPorts : labels.disconnectPort,
        disabled: connectedEdges(options.model, scope).length === 0,
      },
    ];
  const actions = renameItems(scope, options);
  if (scope.nodes.length)
    actions.push({
      id: 'context:disconnect',
      label: labels.disconnect,
      disabled: connectedEdges(options.model, scope).length === 0,
    });
  if (count)
    actions.push({
      id: 'context:remove',
      label:
        count > 1
          ? labels.removeSelection
          : scope.nodes.length
            ? labels.removeNode
            : labels.removeCable,
      destructive: true,
    });
  if (scope.kind !== 'canvas') return actions;
  const items = [...options.addItems];
  if (items.length) items.push({ id: 'context:separator', kind: 'divider', label: '' });
  return [
    ...items,
    { id: 'context:fit', label: labels.fitView, disabled: !options.model.nodes.length },
    { id: 'context:select', label: labels.selectAll, disabled: !options.model.nodes.length },
    {
      id: 'context:clear',
      label: labels.clearSelection,
      disabled: !count && !options.selectedPorts.current.length,
    },
    ...actions,
  ];
}

export function useGraphContextMenu(options: ContextOptions) {
  const [scope, setScope] = useState<Scope>({ kind: 'canvas', nodes: [], edges: [] });
  const select = (nodes: string[], edges: string[]) => {
    const nodeIds = new Set(nodes);
    const edgeIds = new Set(edges);
    options.setNodes((current) =>
      current.map((node) => ({ ...node, selected: nodeIds.has(node.id) })),
    );
    options.setEdges((current) =>
      current.map((edge) => ({ ...edge, selected: edgeIds.has(edge.id) })),
    );
  };
  const prepare = (event: { target: EventTarget | null }) => {
    if (!(event.target instanceof Element)) return;
    const next = resolveScope(event.target, options.nodes, options.edges);
    const clickedPort = next.kind === 'port' ? next.ports[0] : undefined;
    if (
      next.kind === 'port' &&
      clickedPort &&
      options.selectedPorts.current.some(
        (port) =>
          port.nodeId === clickedPort.nodeId &&
          port.portId === clickedPort.portId &&
          port.side === clickedPort.side,
      )
    ) {
      next.ports = [...options.selectedPorts.current];
    }
    setScope(next);
    if (next.kind === 'port') {
      // The highlighted set must be exactly the set the menu will operate on.
      select([], []);
      options.selectPorts(next.ports);
    } else if (next.kind !== 'canvas') {
      select(next.nodes, next.edges);
      options.clearPorts();
    }
  };
  const execute = (id: string): boolean => {
    if (!id.startsWith('context:')) return false;
    const item = contextItems(scope, options).find((item) => item.id === id);
    if (!item || item.disabled) return true;
    switch (id) {
      case 'context:rename':
        (options.renameNode as (nodeId: string) => void)(scope.nodes[0] as string);
        break;
      case 'context:remove':
        options.commit(removeAudioGraphItems(options.model, scope.nodes, scope.edges), 'runtime');
        select([], []);
        break;
      case 'context:disconnect':
        options.commit(
          disconnectAudioGraph(options.model, connectedEdges(options.model, scope)),
          'runtime',
        );
        break;
      case 'context:fit':
        options.fitView();
        break;
      case 'context:select':
        select(
          options.nodes.map((n) => n.id),
          options.edges.map((e) => e.id),
        );
        break;
      case 'context:clear':
        select([], []);
        break;
    }
    options.clearPorts();
    return true;
  };
  const clearSelection = () => {
    select([], []);
    options.clearPorts();
  };
  return { items: contextItems(scope, options), prepare, execute, clearSelection };
}
