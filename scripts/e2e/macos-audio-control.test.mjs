import assert from 'node:assert/strict';
import test from 'node:test';
import { requireRemovedCandidate } from './macos-audio-control.mjs';

test('the independent control requires completed product removal and an original audio verdict', () => {
  for (const status of ['passed', 'failed']) {
    assert.doesNotThrow(() => requireRemovedCandidate({ cleanup: 'passed', scenarios: [{ id: 'AUDIO-01', status }] }));
  }
  assert.throws(() => requireRemovedCandidate({ cleanup: 'failed', scenarios: [{ id: 'AUDIO-01', status: 'failed' }] }), /removal/);
  for (const scenarios of [[], [{ id: 'AUDIO-01', status: 'skipped' }], [{ id: 'GRAPH-01', status: 'passed' }]]) {
    assert.throws(() => requireRemovedCandidate({ cleanup: 'passed', scenarios }), /audio verdict/);
  }
});
