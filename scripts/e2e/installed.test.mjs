import assert from 'node:assert/strict';
import test from 'node:test';
import { signalGraph } from './installed.mjs';

test('unity-level acceptance records the Driver mix without adding a feedback return', () => {
  const original = { schemaVersion: 1, revision: 10, nodes: [], edges: [] };
  const spec = { id: 'default-device', render: { channels: 2 }, capture: { channels: 2 } };
  const graph = signalGraph(original, 'selected-graph', spec);
  const endpoint = graph.nodes.find(node => node.type === 'virtual_endpoint');
  const recorder = graph.nodes.find(node => node.type === 'recorder');
  assert.equal(endpoint.config.id, spec.id);
  assert.deepEqual(endpoint.config.reference, { render: true, capture: true });
  assert.deepEqual(new Set(graph.edges.filter(edge => edge.source.nodeId === endpoint.id && edge.source.portId === 'from_os')
    .map(edge => `${edge.destination.nodeId}/${edge.destination.portId}`)), new Set([`${recorder.id}/audio`]));
  assert.equal(graph.expectedGraphId, 'selected-graph');
  assert.equal(graph.revision, 11);
  assert.deepEqual(original.nodes, []);
});
