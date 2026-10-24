import assert from 'node:assert/strict';
import test from 'node:test';
import { systemCaptureGraph, waitForSystemSignal } from './system-capture.mjs';
import { applyDesktopGraph } from './installed.mjs';

test('system recording waits for both live source channels after first-use consent', async () => {
  let elapsed = 0, reads = 0;
  const windows = [];
  await waitForSystemSignal(async path => {
    assert.equal(path, 'meters'); reads++;
    return { revision: elapsed < 5000 ? 7 : 8, nodes: [
      { nodeId: 'qa-system', portId: 'audio', peaks: elapsed < 6000 ? [0.05, 0] : [0.05, 0.05] },
    ] };
  }, 8, { now: () => elapsed, wait: async ms => { elapsed += ms; }, observe: value => windows.push(value) });
  assert.ok(elapsed >= 6200);
  assert.ok(reads > 3);
  assert.equal(windows.at(-1).revision, 8);
});

test('silent, stale, partial and invalid system signals still fail before recording', async () => {
  for (const [revision, peaks] of [[7, [0.05, 0.05]], [8, [0, 0]], [8, [0.05, 0]], [8, [NaN, 0.05]]]) {
    let elapsed = 0;
    await assert.rejects(waitForSystemSignal(async () => ({ revision, nodes: [
      { nodeId: 'qa-system', portId: 'audio', peaks },
    ] }), 8, { now: () => elapsed, wait: async ms => { elapsed += ms; } }), /did not become live/);
    assert.equal(elapsed, 30000);
  }
});

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
