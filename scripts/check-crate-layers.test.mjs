import assert from 'node:assert/strict';
import test from 'node:test';
import { LAYERS, violation, violations } from './check-crate-layers.mjs';

const metadata = (edges) => ({
  packages: Object.keys(LAYERS).map((name) => ({
    name,
    dependencies: edges
      .filter(([from]) => from === name)
      .map(([, to, kind]) => ({ name: to, kind: kind ?? null })),
  })),
});

test('dependencies follow the documented direction', () => {
  for (const [from, to] of [
    ['mixrelay-audio-graph', 'mixrelay-audio-io'],
    ['mixrelay-control-protocol', 'mixrelay-audio-graph'],
    ['mixrelay-control-server', 'mixrelay-control-protocol'],
    ['mixrelay-hal-core', 'mixrelay-macos-transport'],
    ['mixrelay-linux-driver', 'mixrelay-driver-control'],
    ['mixrelay-engine-service', 'mixrelay-pipewire'],
    ['mixrelay-processor-host', 'truce-rack-vst3'],
  ]) {
    assert.equal(violation(from, to, 'normal'), undefined, `${from} -> ${to}`);
  }
});

test('upward, cross-platform, app and test-support dependencies are rejected', () => {
  assert.match(violation('mixrelay-audio-io', 'mixrelay-audio-graph', 'normal'), /may not depend/);
  assert.match(violation('mixrelay-audio-engine', 'mixrelay-macos-transport', 'normal'), /platform-neutral/);
  assert.match(violation('mixrelay-pipewire', 'mixrelay-macos-transport', 'normal'), /another/);
  assert.match(violation('mixrelay-control-server', 'mixrelay-engine-service', 'normal'), /app/);
  assert.match(violation('mixrelay-desktop', 'mixrelay-engine-service', 'normal'), /app/);
  assert.match(violation('mixrelay-audio-io', 'mixrelay-test-support', 'normal'), /dev-dependency/);
  assert.equal(violation('mixrelay-audio-io', 'mixrelay-test-support', 'dev'), undefined);
  assert.match(violation('mixrelay-audio-engine', 'truce-rack-core', 'normal'), /plugin host/);
  assert.match(violation('mixrelay-new', 'mixrelay-audio-io', 'normal'), /no layer/);
});

test('the report lists each forbidden edge once', () => {
  const found = violations(metadata([
    ['mixrelay-audio-io', 'mixrelay-audio-engine'],
    ['mixrelay-audio-io', 'mixrelay-audio-engine', 'build'],
    ['mixrelay-audio-graph', 'mixrelay-audio-io'],
  ]));
  assert.deepEqual(found, [
    'mixrelay-audio-io -> mixrelay-audio-engine (normal): foundation may not depend on model',
  ]);
});
