import assert from 'node:assert/strict';
import test from 'node:test';
import { CRATES, LAYERS, violation, violations } from './check-crate-layers.mjs';

const metadata = (edges, names = Object.keys(LAYERS)) => ({
  packages: names.map((name) => ({
    name,
    dependencies: edges
      .filter(([from]) => from === name)
      .map(([, to, kind]) => ({ name: to, kind: kind ?? null })),
  })),
});

test('dependencies the table lists are allowed', () => {
  for (const [from, to] of [
    ['mixrelay-audio-graph', 'mixrelay-audio-io'],
    ['mixrelay-audio-engine', 'mixrelay-audio-recorder'],
    ['mixrelay-control-protocol', 'mixrelay-audio-graph'],
    ['mixrelay-control-server', 'mixrelay-control-protocol'],
    ['mixrelay-processor-host', 'mixrelay-control-protocol'],
    ['mixrelay-processor-host', 'truce-rack-vst3'],
    ['mixrelay-hal-core', 'mixrelay-macos-transport'],
    ['mixrelay-hal-core', 'mixrelay-pcm-ring'],
    ['mixrelay-macos-driver', 'mixrelay-driver-control'],
    ['mixrelay-linux-driver', 'mixrelay-driver-control'],
    ['mixrelay-linux-driver', 'mixrelay-pipewire'],
    ['mixrelay-service', 'mixrelay-pipewire'],
    ['mixrelay-service', 'truce-rack-core'],
  ]) {
    assert.equal(violation(from, to, 'normal'), undefined, `${from} -> ${to}`);
  }
});

test('each crate gets exactly its row: no blanket layer allowances', () => {
  // Adapters with no contract in their row may not use one, even though other adapters do.
  for (const from of ['mixrelay-macos-transport', 'mixrelay-hal-core', 'mixrelay-process-tap', 'mixrelay-pipewire']) {
    assert.match(violation(from, 'mixrelay-driver-control', 'normal'), /not in .* allowed/, from);
  }
  assert.match(violation('mixrelay-macos-installer', 'mixrelay-macos-transport', 'normal'), /not in .* allowed/);
  assert.match(violation('mixrelay-hal-bundle', 'mixrelay-audio-io', 'normal'), /not in .* allowed/);
  assert.match(violation('mixrelay-driver-control', 'mixrelay-audio-graph', 'normal'), /not in .* allowed/);
  assert.match(violation('mixrelay-audio-graph', 'mixrelay-audio-recorder', 'normal'), /not in .* allowed/);
  assert.match(violation('mixrelay-control-protocol', 'mixrelay-driver-control', 'normal'), /not in .* allowed/);
  // Build dependencies follow the same row.
  assert.match(violation('mixrelay-audio-io', 'mixrelay-realtime', 'build'), /not in .* allowed/);
});

test('upward, cross-platform, app and test-support dependencies are rejected', () => {
  assert.match(violation('mixrelay-audio-io', 'mixrelay-audio-graph', 'normal'), /not in .* allowed/);
  assert.match(violation('mixrelay-audio-engine', 'mixrelay-macos-transport', 'normal'), /not in .* allowed/);
  assert.match(violation('mixrelay-pipewire', 'mixrelay-macos-transport', 'normal'), /not in .* allowed/);
  assert.match(violation('mixrelay-control-server', 'mixrelay-service', 'normal'), /app/);
  assert.match(violation('mixrelay-desktop', 'mixrelay-service', 'normal'), /app/);
  assert.match(violation('mixrelay-audio-io', 'mixrelay-test-support', 'normal'), /dev-dependency/);
  assert.match(violation('mixrelay-service', 'mixrelay-macos-test-support', 'normal'), /dev-dependency/);
  assert.equal(violation('mixrelay-audio-io', 'mixrelay-test-support', 'dev'), undefined);
  assert.match(violation('mixrelay-audio-engine', 'truce-rack-core', 'normal'), /not in .* allowed/);
  assert.match(violation('mixrelay-control-server', 'truce-rack-core', 'normal'), /not in .* allowed/);
  assert.match(violation('mixrelay-new', 'mixrelay-audio-io', 'normal'), /no row/);
  assert.match(violation('mixrelay-audio-io', 'mixrelay-new', 'normal'), /no row/);
  // Dev dependencies follow the same table; only test support is added.
  assert.match(violation('mixrelay-audio-io', 'mixrelay-desktop', 'dev'), /app/);
  assert.match(violation('mixrelay-audio-graph', 'mixrelay-pipewire', 'dev'), /not in .* allowed/);
  assert.match(violation('mixrelay-test-support', 'mixrelay-audio-io', 'normal'), /not in .* allowed/);
});

test('every workspace crate needs a row, even one with no dependencies, and rows must be members', () => {
  const names = [...Object.keys(LAYERS), 'mixrelay-unregistered'];
  assert.deepEqual(violations(metadata([], names)), [
    'mixrelay-unregistered: workspace crate has no row in docs/architecture.md and scripts/check-crate-layers.mjs',
  ]);
  const missing = Object.keys(LAYERS).filter((name) => name !== 'mixrelay-licensing');
  assert.deepEqual(violations(metadata([], missing)), [
    'mixrelay-licensing: listed in scripts/check-crate-layers.mjs but not a workspace crate',
  ]);
});

test('the report lists each forbidden edge once and names its dependency kind', () => {
  const found = violations(metadata([
    ['mixrelay-audio-io', 'mixrelay-audio-engine'],
    ['mixrelay-audio-io', 'mixrelay-audio-engine', 'build'],
    ['mixrelay-audio-graph', 'mixrelay-audio-io'],
    ['mixrelay-hal-core', 'mixrelay-driver-control', 'dev'],
  ]));
  assert.deepEqual(found, [
    "mixrelay-audio-io -> mixrelay-audio-engine (build): not in mixrelay-audio-io's allowed dependencies in docs/architecture.md",
    "mixrelay-audio-io -> mixrelay-audio-engine (normal): not in mixrelay-audio-io's allowed dependencies in docs/architecture.md",
    "mixrelay-hal-core -> mixrelay-driver-control (dev): not in mixrelay-hal-core's allowed dependencies in docs/architecture.md",
  ]);
});

test('allow-lists only name registered crates', () => {
  for (const [name, row] of Object.entries(CRATES)) {
    if (!Array.isArray(row.allows)) continue;
    for (const to of row.allows) assert.ok(CRATES[to], `${name} allows unregistered ${to}`);
  }
});
