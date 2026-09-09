import { act, renderHook } from '@testing-library/react';
import { useRef, useState } from 'react';
import { expect, it, vi } from 'vitest';
import { useUndoHistory } from '../history/useUndoHistory';
import type { AudioGraphModel } from './model';
import {
  type AudioFlowEdge,
  type AudioFlowNode,
  reconcileAudioFlowEdges,
  reconcileAudioFlowNodes,
} from './reactFlowAdapter';
import { useGraphContextMenu } from './useGraphContextMenu';

const model: AudioGraphModel = {
  revision: 1,
  nodes: ['a', 'b', 'c'].map((id) => ({
    id,
    position: { x: 0, y: 0 },
    appearance: { title: id, category: '', icon: '' },
    inputPorts: [{ id: 'in', label: 'Input' }],
    outputPorts: [{ id: 'out', label: 'Output' }],
  })),
  edges: [
    { id: 'ab', sourceNodeId: 'a', sourcePortId: 'out', targetNodeId: 'b', targetPortId: 'in' },
    { id: 'cb', sourceNodeId: 'c', sourcePortId: 'out', targetNodeId: 'b', targetPortId: 'in' },
  ],
};
type Port = { nodeId: string; portId: string; side: 'input' | 'output' };
function target(
  kind: 'node' | 'edge' | 'port' | 'selection' | 'canvas',
  id = 'a',
  side: 'input' | 'output' = 'output',
) {
  const element = document.createElement('div');
  if (kind === 'node' || kind === 'edge') {
    element.className = `react-flow__${kind}`;
    element.dataset.id = id;
  }
  if (kind === 'port')
    Object.assign(element.dataset, {
      audioNodeId: id,
      audioPortId: side === 'input' ? 'in' : 'out',
      audioPortSide: side,
    });
  if (kind === 'selection') element.className = 'react-flow__nodesselection-rect';
  return { target: element };
}
function setup(selectedNodes: string[] = [], selectedEdges: string[] = [], ports: Port[] = []) {
  const committed = vi.fn();
  const fitView = vi.fn();
  const selectPorts = vi.fn();
  const hook = renderHook(() => {
    const [current, setModel] = useState(model);
    const [nodes, setNodes] = useState<AudioFlowNode[]>(() =>
      reconcileAudioFlowNodes([], model).map((n) => ({
        ...n,
        selected: selectedNodes.includes(n.id),
      })),
    );
    const [edges, setEdges] = useState<AudioFlowEdge[]>(() =>
      reconcileAudioFlowEdges([], model).map((e) => ({
        ...e,
        selected: selectedEdges.includes(e.id),
      })),
    );
    const selectedPorts = useRef(ports);
    const history = useUndoHistory<AudioGraphModel, 'runtime'>();
    const menu = useGraphContextMenu({
      model: current,
      nodes,
      edges,
      setNodes,
      setEdges,
      selectedPorts,
      fitView,
      addItems: [{ id: 'source', label: 'Source' }],
      selectPorts: (ports) => {
        selectedPorts.current = [...ports];
        selectPorts(ports);
      },
      clearPorts: () => {
        selectedPorts.current = [];
      },
      commit: (next, kind) => {
        history.record(current, next, kind);
        setModel(next);
        committed(next, kind);
      },
    });
    return { menu, nodes, edges, current, selectedPorts, history };
  });
  return { ...hook, committed, fitView, selectPorts };
}

it('deletes a node and all incident cables in one undoable mutation', () => {
  const f = setup();
  act(() => f.result.current.menu.prepare(target('node', 'b')));
  act(() => f.result.current.menu.execute('context:remove'));
  expect(f.committed).toHaveBeenCalledTimes(1);
  expect(f.result.current.current.nodes.map((n) => n.id)).toEqual(['a', 'c']);
  expect(f.result.current.current.edges).toEqual([]);
  expect(f.result.current.current.revision).toBe(2);
  expect(f.result.current.history.undo()?.value).toEqual(model);
  expect(f.result.current.history.undo()).toBeNull();
  expect(f.result.current.history.redo()?.value.nodes.map((n) => n.id)).toEqual(['a', 'c']);
});

it('preserves selected nodes and cables when right-clicking the selection rectangle', () => {
  const f = setup(['a', 'c'], ['cb']);
  act(() => f.result.current.menu.prepare(target('selection')));
  expect(f.result.current.menu.items.map((i) => i.id)).toEqual([
    'context:disconnect',
    'context:remove',
  ]);
  act(() => f.result.current.menu.execute('context:remove'));
  expect(f.result.current.current.nodes.map((n) => n.id)).toEqual(['b']);
  expect(f.result.current.current.edges).toEqual([]);
});

it('right-clicking outside the selected nodes scopes deletion to that node only', () => {
  const f = setup(['a', 'c']);
  act(() => f.result.current.menu.prepare(target('node', 'b')));
  expect(f.result.current.nodes.filter((n) => n.selected).map((n) => n.id)).toEqual(['b']);
  act(() => f.result.current.menu.execute('context:remove'));
  expect(f.result.current.current.nodes.map((n) => n.id)).toEqual(['a', 'c']);
});

it('deletes selected cables without removing their nodes', () => {
  const f = setup([], ['ab', 'cb']);
  act(() => f.result.current.menu.prepare(target('edge', 'ab')));
  act(() => f.result.current.menu.execute('context:remove'));
  expect(f.result.current.current.nodes).toEqual(model.nodes);
  expect(f.result.current.current.edges).toEqual([]);
});

it('disconnects node cables while preserving all nodes and unrelated edges', () => {
  const f = setup();
  act(() => f.result.current.menu.prepare(target('node', 'a')));
  act(() => f.result.current.menu.execute('context:disconnect'));
  expect(f.result.current.current.nodes).toEqual(model.nodes);
  expect(f.result.current.current.edges.map((e) => e.id)).toEqual(['cb']);
});

it.each(['input', 'output'] as const)('disconnects the exact %s channel', (side) => {
  const f = setup();
  act(() => f.result.current.menu.prepare(target('port', side === 'input' ? 'b' : 'a', side)));
  act(() => f.result.current.menu.execute('context:disconnect'));
  expect(f.result.current.current.edges.map((e) => e.id)).toEqual(side === 'input' ? [] : ['cb']);
});

it('keeps the selected port group as the exact execution and highlight set', () => {
  const ports: Port[] = ['a', 'c'].map((nodeId) => ({ nodeId, portId: 'out', side: 'output' }));
  const f = setup(['b'], [], ports);
  act(() => f.result.current.menu.prepare(target('port', 'a')));
  expect(f.selectPorts).toHaveBeenCalledWith(ports);
  expect(f.result.current.nodes.some((n) => n.selected)).toBe(false);
  act(() => f.result.current.menu.execute('context:disconnect'));
  expect(f.result.current.current.edges).toEqual([]);
  expect(f.committed).toHaveBeenCalledTimes(1);
});

it('replaces old port highlights when right-clicking an unselected port', () => {
  const f = setup(['b'], [], [{ nodeId: 'c', portId: 'out', side: 'output' }]);
  act(() => f.result.current.menu.prepare(target('port', 'a')));
  expect(f.selectPorts).toHaveBeenCalledWith([{ nodeId: 'a', portId: 'out', side: 'output' }]);
  act(() => f.result.current.menu.execute('context:disconnect'));
  expect(f.result.current.current.edges.map((e) => e.id)).toEqual(['cb']);
});

it('guards disabled commands and leaves node-add ids to the creation handler', () => {
  const f = setup();
  act(() => f.result.current.menu.prepare(target('port', 'c', 'input')));
  expect(f.result.current.menu.items[0]?.disabled).toBe(true);
  act(() => f.result.current.menu.execute('context:disconnect'));
  expect(f.committed).not.toHaveBeenCalled();
  expect(f.result.current.menu.execute('source')).toBe(false);
});

it('selects all, clears all and fits the viewport without recording a graph mutation', () => {
  const f = setup();
  act(() => f.result.current.menu.prepare(target('canvas')));
  act(() => f.result.current.menu.execute('context:select'));
  expect(f.result.current.nodes.every((n) => n.selected)).toBe(true);
  expect(f.result.current.edges.every((e) => e.selected)).toBe(true);
  act(() => f.result.current.menu.prepare(target('canvas')));
  act(() => f.result.current.menu.execute('context:clear'));
  act(() => f.result.current.menu.execute('context:fit'));
  expect(f.result.current.nodes.some((n) => n.selected)).toBe(false);
  expect(f.result.current.edges.some((e) => e.selected)).toBe(false);
  expect(f.fitView).toHaveBeenCalledOnce();
  expect(f.committed).not.toHaveBeenCalled();
});
