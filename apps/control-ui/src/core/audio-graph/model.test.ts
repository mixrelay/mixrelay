import { expect, it } from 'vitest';

import {
  type AudioGraphConnection,
  type AudioGraphModel,
  type AudioGraphNode,
  addAudioGraphNode,
  canConnectAudioGraph,
  connectAudioGraph,
  connectAudioGraphBatch,
  disconnectAudioGraph,
  moveAudioGraphNode,
  reconnectAudioGraph,
  removeAudioGraphItems,
} from './model';

function node(id: string, inputPorts: string[], outputPorts: string[]): AudioGraphNode {
  return {
    id,
    position: { x: 0, y: 0 },
    inputPorts: inputPorts.map((portId) => ({ id: portId, label: portId })),
    outputPorts: outputPorts.map((portId) => ({ id: portId, label: portId })),
    appearance: { category: 'Test', detail: id, icon: 'T', title: id },
  };
}

function connection(
  sourceNodeId: string,
  targetNodeId: string,
  sourcePortId = 'out',
  targetPortId = 'in',
): AudioGraphConnection {
  return { sourceNodeId, sourcePortId, targetNodeId, targetPortId };
}

function emptyModel(): AudioGraphModel {
  return {
    revision: 4,
    nodes: [node('a', [], ['out']), node('b', ['in'], ['out']), node('c', ['in'], [])],
    edges: [],
  };
}

it('connects only valid output-to-input routes', () => {
  const model = emptyModel();
  const valid = connection('a', 'b');
  expect(canConnectAudioGraph(model, valid)).toBe(true);
  expect(canConnectAudioGraph(model, connection('a', 'a'))).toBe(false);
  expect(canConnectAudioGraph(model, { ...valid, sourcePortId: '' })).toBe(false);
  expect(canConnectAudioGraph(model, { ...valid, targetPortId: '' })).toBe(false);
  expect(canConnectAudioGraph(model, connection('missing', 'b'))).toBe(false);
  expect(canConnectAudioGraph(model, connection('a', 'missing'))).toBe(false);
  expect(canConnectAudioGraph(model, connection('a', 'b', 'missing'))).toBe(false);
  expect(canConnectAudioGraph(model, connection('a', 'b', 'out', 'missing'))).toBe(false);

  const connected = connectAudioGraph(model, valid);
  expect(connected).not.toBe(model);
  expect(connected.revision).toBe(5);
  expect(connected.edges).toEqual([
    {
      id: 'edge:a:out:b:in',
      ...valid,
    },
  ]);
  expect(connectAudioGraph(connected, valid)).toBe(connected);
  expect(canConnectAudioGraph(connected, connection('b', 'b'))).toBe(false);
  expect(canConnectAudioGraph(connected, connection('b', 'b', 'out', 'in'))).toBe(false);
});

it('allows input fan-in while rejecting duplicate edges and processing cycles', () => {
  const aToB = connectAudioGraph(emptyModel(), connection('a', 'b'));
  expect(canConnectAudioGraph(aToB, connection('a', 'b'))).toBe(false);
  expect(canConnectAudioGraph(aToB, connection('b', 'b'))).toBe(false);

  const bToC = connectAudioGraph(aToB, connection('b', 'c'));
  const withCyclePort: AudioGraphModel = {
    ...bToC,
    nodes: bToC.nodes.map((item) =>
      item.id === 'a'
        ? { ...item, inputPorts: [{ id: 'in', label: 'in' }] }
        : item.id === 'c'
          ? { ...item, outputPorts: [{ id: 'out', label: 'out' }] }
          : item,
    ),
  };
  expect(canConnectAudioGraph(withCyclePort, connection('c', 'a'))).toBe(false);

  const occupiedTarget = {
    ...emptyModel(),
    nodes: [...emptyModel().nodes, node('d', [], ['out'])],
    edges: [
      {
        id: 'existing',
        ...connection('a', 'b'),
      },
    ],
  };
  expect(canConnectAudioGraph(occupiedTarget, connection('d', 'b'))).toBe(true);
  const fanIn = connectAudioGraph(occupiedTarget, connection('d', 'b'));
  expect(fanIn.edges).toHaveLength(2);
  expect(fanIn.edges[0]).toEqual(occupiedTarget.edges[0]);
  expect(fanIn.edges[1]).toMatchObject(connection('d', 'b'));

  const convergingModel: AudioGraphModel = {
    revision: 1,
    nodes: [
      node('source', [], ['out']),
      node('branch', ['in'], ['out']),
      node('left', ['in'], ['out']),
      node('right', ['in'], ['out']),
      node('join', ['left', 'right'], []),
    ],
    edges: [
      { id: 'branch-left', ...connection('branch', 'left') },
      { id: 'branch-right', ...connection('branch', 'right') },
      { id: 'left-join', ...connection('left', 'join', 'out', 'left') },
      { id: 'right-join', ...connection('right', 'join', 'out', 'right') },
    ],
  };
  expect(canConnectAudioGraph(convergingModel, connection('source', 'branch'))).toBe(true);
});

it('treats host source and sink ports as graph boundaries instead of a processing cycle', () => {
  const endpoint = node('virtual-io', ['to_os'], ['from_os']);
  endpoint.inputPorts[0] = { boundary: 'sink', id: 'to_os', label: 'to_os' };
  endpoint.outputPorts[0] = { boundary: 'source', id: 'from_os', label: 'from_os' };
  const model: AudioGraphModel = { revision: 1, nodes: [endpoint], edges: [] };

  expect(
    canConnectAudioGraph(model, connection('virtual-io', 'virtual-io', 'from_os', 'to_os')),
  ).toBe(true);
  expect(
    connectAudioGraph(model, connection('virtual-io', 'virtual-io', 'from_os', 'to_os')).edges,
  ).toHaveLength(1);

  const withProcessors: AudioGraphModel = {
    revision: 1,
    nodes: [endpoint, node('left', ['in'], ['out']), node('right', ['in'], ['out'])],
    edges: [
      {
        id: 'host-boundary',
        ...connection('virtual-io', 'virtual-io', 'from_os', 'to_os'),
      },
    ],
  };
  expect(canConnectAudioGraph(withProcessors, connection('left', 'right'))).toBe(true);
});

it('reconnects atomically while preserving the edge identity', () => {
  const connected = connectAudioGraph(emptyModel(), connection('a', 'b'));
  const edgeId = connected.edges[0]?.id ?? '';
  expect(reconnectAudioGraph(connected, 'missing', connection('b', 'c'))).toBe(connected);
  expect(reconnectAudioGraph(connected, edgeId, connection('b', 'b'))).toBe(connected);

  const reconnected = reconnectAudioGraph(connected, edgeId, connection('b', 'c'));
  expect(reconnected.revision).toBe(6);
  expect(reconnected.edges).toEqual([{ id: edgeId, ...connection('b', 'c') }]);

  const twoEdges = connectAudioGraph(connected, connection('b', 'c'));
  const sameEndpoint = reconnectAudioGraph(twoEdges, edgeId, connection('a', 'b'));
  expect(sameEndpoint.edges).toHaveLength(2);
  expect(sameEndpoint.edges[1]).toBe(twoEdges.edges[1]);
});

it('disconnects batches and moves nodes without changing the audio revision', () => {
  const connected = connectAudioGraph(emptyModel(), connection('a', 'b'));
  expect(disconnectAudioGraph(connected, ['missing'])).toBe(connected);
  const disconnected = disconnectAudioGraph(connected, [connected.edges[0]?.id ?? '']);
  expect(disconnected.edges).toEqual([]);
  expect(disconnected.revision).toBe(6);

  expect(moveAudioGraphNode(disconnected, 'missing', { x: 10, y: 20 })).toBe(disconnected);
  const moved = moveAudioGraphNode(disconnected, 'b', { x: 10, y: 20 });
  expect(moved.revision).toBe(disconnected.revision);
  expect(moved.nodes.find((item) => item.id === 'b')?.position).toEqual({ x: 10, y: 20 });
  expect(moveAudioGraphNode(moved, 'b', { x: 10, y: 20 })).toBe(moved);
});

it('adds only valid unique nodes and removes nodes with incident edges atomically', () => {
  const model = emptyModel();
  const valid = node('d', ['left', 'right'], ['out']);
  expect(addAudioGraphNode(model, valid).nodes.at(-1)).toBe(valid);
  expect(addAudioGraphNode(model, node('a', [], []))).toBe(model);
  expect(addAudioGraphNode(model, node('', [], []))).toBe(model);
  expect(addAudioGraphNode(model, node('d', ['', 'right'], []))).toBe(model);
  expect(addAudioGraphNode(model, node('d', ['same', 'same'], []))).toBe(model);
  expect(addAudioGraphNode(model, node('d', [], ['']))).toBe(model);
  expect(addAudioGraphNode(model, node('d', [], ['same', 'same']))).toBe(model);

  const connected = connectAudioGraph(model, connection('a', 'b'));
  expect(removeAudioGraphItems(connected, [])).toBe(connected);
  expect(removeAudioGraphItems(connected, ['missing'], ['missing'])).toBe(connected);
  const withoutNode = removeAudioGraphItems(connected, ['b']);
  expect(withoutNode.nodes.some((item) => item.id === 'b')).toBe(false);
  expect(withoutNode.edges).toEqual([]);
  expect(withoutNode.revision).toBe(6);

  const withoutEdge = removeAudioGraphItems(connected, [], [connected.edges[0]?.id ?? '']);
  expect(withoutEdge.nodes).toBe(connected.nodes);
  expect(withoutEdge.edges).toEqual([]);
});

it('encodes endpoint components into deterministic edge IDs', () => {
  const model: AudioGraphModel = {
    revision: 0,
    nodes: [node('source / one', [], ['out 1']), node('target', ['in/1'], [])],
    edges: [],
  };
  expect(
    connectAudioGraph(model, connection('source / one', 'target', 'out 1', 'in/1')).edges[0]?.id,
  ).toBe('edge:source%20%2F%20one:out%201:target:in%2F1');
});

it('keeps the same graph when a channel batch contains no connections', () => {
  const model = emptyModel();
  expect(connectAudioGraphBatch(model, [])).toBe(model);
  expect(model.revision).toBe(4);
});
