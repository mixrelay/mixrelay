import assert from 'node:assert/strict';
import test from 'node:test';
import { systemCaptureGraph } from './system-capture.mjs';
import { applyDesktopGraph } from './installed.mjs';

test('whole-system acceptance submits the complete node representation returned by the desktop', async () => {
  const graph = systemCaptureGraph({ schemaVersion: 1, revision: 7, nodes: [], edges: [] }, 'whole-system');
  const observed = { ...graph, nodes: graph.nodes.map(node => ({ ...node,
    label: node.label ?? null, control: { enabled: true, muted: false, ...node.control } })) };
  const result = await applyDesktopGraph(graph, {
    invoke: async () => ({ status: 200, body: { protocolVersion: 1, revision: 8, state: 'active' } }),
    active: async () => {}, api: async () => observed,
  });
  assert.equal(result, observed);
  assert.equal(graph.nodes[0].config.selector.match, 'all_except');
  assert.equal(graph.nodes[0].config.sourcePlayback, 'preserve');
  assert.equal(graph.edges[0].destination.nodeId, graph.nodes[1].id);
  await assert.rejects(applyDesktopGraph(graph, {
    invoke: async () => ({ status: 200, body: { protocolVersion: 1, revision: 8, state: 'active' } }),
    active: async () => {}, api: async () => ({ ...observed, nodes: observed.nodes.map(node => ({
      ...node, control: { enabled: false, muted: true },
    })) }),
  }), /Expected values to be strictly deep-equal/);
});
