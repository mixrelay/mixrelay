import { assert, describe, expect, it } from 'vitest';

import {
  type AudioGraphModel,
  connectAudioGraph,
  disconnectAudioGraph,
  resizeAudioGraphNode,
} from './model';
import { reconcileAudioFlowEdges, reconcileAudioFlowNodes } from './reactFlowAdapter';

function largeGraph(size: number): AudioGraphModel {
  return {
    revision: 1,
    nodes: Array.from({ length: size }, (_, index) => ({
      id: `node-${index}`,
      position: { x: index * 20, y: 0 },
      inputPorts: index === 0 ? [] : [{ id: 'in', label: 'Input' }],
      outputPorts: index === size - 1 ? [] : [{ id: 'out', label: 'Output' }],
      appearance: { category: 'Test', detail: `${index}`, icon: 'T', title: `${index}` },
    })),
    edges: Array.from({ length: size - 1 }, (_, index) => ({
      id: `edge-${index}`,
      sourceNodeId: `node-${index}`,
      sourcePortId: 'out',
      targetNodeId: `node-${index + 1}`,
      targetPortId: 'in',
    })),
  };
}

describe('React Flow incremental adapter', () => {
  it('restores presentation sizes without changing the audio revision or other nodes', () => {
    const before = largeGraph(2);
    const flow = reconcileAudioFlowNodes([], before);
    const first = flow[0];
    const second = flow[1];
    assert(first && second);
    const after = resizeAudioGraphNode(before, 'node-0', { width: 420, height: 280 });
    const measured = { ...first, height: 444, measured: { width: 420, height: 444 } };
    const resized = reconcileAudioFlowNodes([measured, second], after);
    expect(after.revision).toBe(before.revision);
    expect(resized[0]).toMatchObject({ width: 420, measured: { height: 444 } });
    expect(resized[0]?.height).toBeUndefined();
    expect(resized[0]?.data.graphNode.size).toEqual({ width: 420, height: 280 });
    expect(resized[1]).toBe(flow[1]);
    expect(resizeAudioGraphNode(after, 'node-0', { width: 420, height: 280 })).toBe(after);
    expect(resizeAudioGraphNode(after, 'node-0', { width: NaN, height: 280 })).toBe(after);
    const bounded = resizeAudioGraphNode(after, 'node-0', { width: 1, height: 5000 });
    expect(bounded.nodes[0]?.size).toEqual({ width: 180, height: 4096 });
  });

  it('preserves every unaffected adapter during an edge disconnect', () => {
    const before = largeGraph(1_024);
    const nodes = reconcileAudioFlowNodes([], before);
    const edges = reconcileAudioFlowEdges([], before);
    expect(edges[0]?.type).toBe('default');
    expect(edges[0]?.zIndex).toBe(0);
    expect(nodes[0]?.zIndex).toBe(1);
    expect(reconcileAudioFlowEdges(edges, before)).toBe(edges);
    const after = disconnectAudioGraph(before, ['edge-511']);
    const nextNodes = reconcileAudioFlowNodes(nodes, after);
    const nextEdges = reconcileAudioFlowEdges(edges, after);

    expect(nextNodes).toBe(nodes);
    expect(nextNodes.every((node, index) => node === nodes[index])).toBe(true);
    expect(nextEdges).toHaveLength(1_022);
    expect(nextEdges.every((edge) => edges.includes(edge))).toBe(true);
  });

  it('allocates only the newly connected edge and keeps transient selection state', () => {
    const before: AudioGraphModel = {
      ...largeGraph(3),
      edges: [],
    };
    const nodes = reconcileAudioFlowNodes([], before);
    const selectedNodes = nodes.map((node, index) =>
      index === 1 ? { ...node, selected: true } : node,
    );
    const after = connectAudioGraph(before, {
      sourceNodeId: 'node-0',
      sourcePortId: 'out',
      targetNodeId: 'node-1',
      targetPortId: 'in',
    });
    const nextNodes = reconcileAudioFlowNodes(selectedNodes, after);
    const nextEdges = reconcileAudioFlowEdges([], after);

    expect(nextNodes).toBe(selectedNodes);
    expect(nextNodes[1]).toBe(selectedNodes[1]);
    expect(nextNodes[1]?.selected).toBe(true);
    expect(nextEdges).toHaveLength(1);
  });

  it('replaces only the adapter whose domain object changed', () => {
    const before = largeGraph(4);
    const nodes = reconcileAudioFlowNodes([], before);
    const changed: AudioGraphModel = {
      ...before,
      nodes: before.nodes.map((node, index) =>
        index === 2 ? { ...node, position: { x: 999, y: 20 } } : node,
      ),
    };
    const nextNodes = reconcileAudioFlowNodes(nodes, changed);

    expect(nextNodes[0]).toBe(nodes[0]);
    expect(nextNodes[1]).toBe(nodes[1]);
    expect(nextNodes[2]).not.toBe(nodes[2]);
    expect(nextNodes[3]).toBe(nodes[3]);
  });
});
