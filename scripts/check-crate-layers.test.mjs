import assert from 'node:assert/strict';
import test from 'node:test';
import { CRATES, LAYERS, engineAdapter, violation, violations } from './check-crate-layers.mjs';

const metadata = (edges, names = Object.keys(LAYERS)) => ({
  packages: names.map((name) => ({
    name,
    dependencies: edges
      .filter(([from]) => from === name)
      .map(([, to, kind, features]) => ({ name: to, kind: kind ?? null, features: features ?? [] })),
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
    ['mixrelay-service', 'mixrelay-engine'],
    ['mixrelay-audio-engine', 'mixrelay-time'],
    ['mixrelay-engine', 'mixrelay-time'],
    ['mixrelay-engine', 'mixrelay-audio-graph'],
    ['mixrelay-engine', 'mixrelay-control-protocol'],
    ['mixrelay-engine', 'mixrelay-driver-control'],
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

test('the engine domain sits above model and contract crates and below adapters and apps', () => {
  for (const to of ['mixrelay-macos-transport', 'mixrelay-pipewire', 'mixrelay-control-server', 'mixrelay-processor-host', 'truce-rack-core']) {
    assert.match(violation('mixrelay-engine', to, 'normal'), /not in .* allowed/, to);
  }
  assert.match(violation('mixrelay-engine', 'mixrelay-service', 'normal'), /app/);
  // Inner crates define the ports they use instead of reaching up to the domain crate.
  for (const from of ['mixrelay-time', 'mixrelay-audio-engine', 'mixrelay-control-protocol', 'mixrelay-control-server', 'mixrelay-processor-host']) {
    assert.match(violation(from, 'mixrelay-engine', 'normal'), /not in .* allowed/, from);
  }
  assert.match(violation('mixrelay-time', 'mixrelay-audio-io', 'normal'), /not in .* allowed/);
});

test('engine adapters may use mixrelay-engine; other adapters may not', () => {
  // No engine adapter exists yet, so check rows shaped like the ones that will be added.
  const crates = {
    ...CRATES,
    'mixrelay-macos-engine': { layer: 'macos', allows: engineAdapter('mixrelay-macos-transport', 'mixrelay-process-tap') },
    'mixrelay-linux-engine': { layer: 'linux', allows: engineAdapter('mixrelay-pipewire') },
  };
  for (const [from, to] of [
    ['mixrelay-macos-engine', 'mixrelay-engine'],
    ['mixrelay-macos-engine', 'mixrelay-time'],
    ['mixrelay-macos-engine', 'mixrelay-audio-graph'],
    ['mixrelay-macos-engine', 'mixrelay-driver-control'],
    ['mixrelay-macos-engine', 'mixrelay-process-tap'],
    ['mixrelay-linux-engine', 'mixrelay-engine'],
    ['mixrelay-linux-engine', 'mixrelay-pipewire'],
    ['mixrelay-service', 'mixrelay-macos-engine'],
  ]) {
    assert.equal(violation(from, to, 'normal', crates), undefined, `${from} -> ${to}`);
  }
  assert.match(violation('mixrelay-linux-engine', 'mixrelay-macos-transport', 'normal', crates), /not in .* allowed/);
  assert.match(violation('mixrelay-macos-engine', 'mixrelay-control-server', 'normal', crates), /not in .* allowed/);
  assert.match(violation('mixrelay-engine', 'mixrelay-macos-engine', 'normal', crates), /not in .* allowed/);
  for (const from of ['mixrelay-macos-transport', 'mixrelay-hal-core', 'mixrelay-process-tap', 'mixrelay-macos-driver', 'mixrelay-pipewire', 'mixrelay-linux-driver']) {
    assert.match(violation(from, 'mixrelay-engine', 'normal'), /not in .* allowed/, from);
  }
});

test('test-* features of workspace crates are enabled only from dev-dependencies', () => {
  const found = violations(metadata([
    ['mixrelay-engine', 'mixrelay-time', 'dev', ['test-util']],
    ['mixrelay-hal-core', 'mixrelay-macos-transport', 'dev', ['test-observation']],
    ['mixrelay-engine', 'mixrelay-time', null, ['test-util']],
    ['mixrelay-hal-core', 'mixrelay-macos-transport', 'build', ['test-observation']],
    // External crates own their feature policy.
    ['mixrelay-desktop', 'tokio', null, ['test-util']],
  ]));
  assert.deepEqual(found, [
    'mixrelay-engine -> mixrelay-time (normal): test feature test-util is for dev-dependencies only',
    'mixrelay-hal-core -> mixrelay-macos-transport (build): test feature test-observation is for dev-dependencies only',
  ]);
});
