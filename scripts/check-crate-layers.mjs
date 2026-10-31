#!/usr/bin/env node
// Checks that workspace crates depend only on what docs/architecture.md allows.
// Reads `cargo metadata`; prints each forbidden dependency and each workspace crate missing
// from the table, and exits 1 when any exist. Normal, build and dev dependencies all follow
// the table; dev dependencies may additionally use test support.
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

const FOUNDATION = [
  'mixrelay-audio-io',
  'mixrelay-pcm-ring',
  'mixrelay-realtime',
  'mixrelay-local-socket',
  'mixrelay-licensing',
  'mixrelay-processor-state',
  'mixrelay-engine-process',
  'mixrelay-midi-control',
];
const MODELS = ['mixrelay-audio-graph', 'mixrelay-audio-recorder', 'mixrelay-audio-engine'];
const CONTRACTS = ['mixrelay-control-protocol', 'mixrelay-driver-control'];
const PLUGIN_FORMATS = ['truce-rack-core', 'truce-rack-clap', 'truce-rack-vst3'];
/** Every non-app crate; apps may use any of them ("앱 외 전부"). Test support stays dev-only. */
const ALL_BUT_APPS = Symbol('all but apps');

/**
 * The table in docs/architecture.md, one row per crate: its layer and the exact workspace
 * crates it may depend on. Same-layer and cross-layer allowances are listed per crate, so a
 * new edge needs a table change here and in the document.
 */
export const CRATES = {
  'mixrelay-midi-control': { layer: 'foundation', allows: [] },
  'mixrelay-audio-io': { layer: 'foundation', allows: [] },
  'mixrelay-pcm-ring': { layer: 'foundation', allows: [] },
  'mixrelay-realtime': { layer: 'foundation', allows: [] },
  'mixrelay-local-socket': { layer: 'foundation', allows: [] },
  'mixrelay-licensing': { layer: 'foundation', allows: [] },
  'mixrelay-processor-state': { layer: 'foundation', allows: [] },
  'mixrelay-engine-process': { layer: 'foundation', allows: [] },
  'mixrelay-audio-graph': { layer: 'model', allows: FOUNDATION },
  'mixrelay-audio-recorder': { layer: 'model', allows: FOUNDATION },
  'mixrelay-audio-engine': { layer: 'model', allows: [...FOUNDATION, 'mixrelay-audio-graph', 'mixrelay-audio-recorder'] },
  'mixrelay-control-protocol': { layer: 'contract', allows: [...FOUNDATION, ...MODELS] },
  'mixrelay-driver-control': { layer: 'contract', allows: FOUNDATION },
  'mixrelay-control-server': { layer: 'service', allows: [...FOUNDATION, ...MODELS, ...CONTRACTS] },
  // The table's known exception: plugin descriptors and editor frames live in the protocol.
  'mixrelay-processor-host': { layer: 'service', allows: [...FOUNDATION, ...MODELS, 'mixrelay-control-protocol', ...PLUGIN_FORMATS] },
  'mixrelay-macos-transport': { layer: 'macos', allows: FOUNDATION },
  'mixrelay-hal-core': { layer: 'macos', allows: [...FOUNDATION, 'mixrelay-macos-transport'] },
  'mixrelay-process-tap': { layer: 'macos', allows: [...FOUNDATION, 'mixrelay-macos-transport'] },
  'mixrelay-macos-driver': { layer: 'macos', allows: [...FOUNDATION, ...CONTRACTS, 'mixrelay-macos-transport'] },
  'mixrelay-macos-installer': { layer: 'macos', allows: [...FOUNDATION, ...CONTRACTS] },
  'mixrelay-hal-bundle': { layer: 'macos', allows: [] },
  'mixrelay-pipewire': { layer: 'linux', allows: FOUNDATION },
  'mixrelay-linux-driver': { layer: 'linux', allows: [...FOUNDATION, ...CONTRACTS, 'mixrelay-pipewire'] },
  'mixrelay-engine-service': { layer: 'app', allows: ALL_BUT_APPS },
  'mixrelay-driver-service': { layer: 'app', allows: ALL_BUT_APPS },
  'mixrelay-desktop': { layer: 'app', allows: ALL_BUT_APPS },
  'mixrelay-test-support': { layer: 'test', allows: [] },
  'mixrelay-macos-test-support': { layer: 'test', allows: [] },
  // tests/fixtures/*
  'mixrelay-native-clap-fixture': { layer: 'test', allows: [] },
  // A diagnostic preload with no workspace deps.
  'mixrelay-pipewire-observer': { layer: 'test', allows: [] },
  // Vendored plugin formats.
  'truce-rack-core': { layer: 'vendor', allows: [] },
  'truce-rack-clap': { layer: 'vendor', allows: ['truce-rack-core'] },
  'truce-rack-vst3': { layer: 'vendor', allows: ['truce-rack-core'] },
};

/** Layer of each registered crate. */
export const LAYERS = Object.fromEntries(Object.entries(CRATES).map(([name, row]) => [name, row.layer]));

function allowed(from, to) {
  const { allows } = CRATES[from];
  if (allows === ALL_BUT_APPS) return !['app', 'test'].includes(CRATES[to].layer);
  return allows.includes(to);
}

/** Why `from -> to` is forbidden, or undefined when the table allows it. */
export function violation(from, to, kind) {
  if (CRATES[from] === undefined) return `${from} has no row in scripts/check-crate-layers.mjs (docs/architecture.md table)`;
  if (CRATES[to] === undefined) return `${to} has no row in scripts/check-crate-layers.mjs (docs/architecture.md table)`;
  if (CRATES[to].layer === 'test') return kind === 'dev' ? undefined : 'test support is a dev-dependency only';
  if (CRATES[to].layer === 'app') return 'nothing depends on an app';
  if (allowed(from, to)) return undefined;
  return `not in ${from}'s allowed dependencies in docs/architecture.md`;
}

/** Forbidden dependencies and unregistered or stale crates in `cargo metadata` output. */
export function violations(metadata) {
  const members = new Set(metadata.packages.map((item) => item.name));
  const found = [];
  for (const item of metadata.packages) {
    if (CRATES[item.name] === undefined) {
      found.push(`${item.name}: workspace crate has no row in docs/architecture.md and scripts/check-crate-layers.mjs`);
    }
    for (const dependency of item.dependencies) {
      if (!members.has(dependency.name)) continue;
      const kind = dependency.kind === 'dev' ? 'dev' : dependency.kind === 'build' ? 'build' : 'normal';
      const reason = violation(item.name, dependency.name, kind);
      if (reason) found.push(`${item.name} -> ${dependency.name} (${kind}): ${reason}`);
    }
  }
  for (const name of Object.keys(CRATES)) {
    if (!members.has(name)) found.push(`${name}: listed in scripts/check-crate-layers.mjs but not a workspace crate`);
  }
  return [...new Set(found)].sort();
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const metadata = JSON.parse(
    execFileSync('cargo', ['metadata', '--format-version', '1', '--no-deps'], {
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    }),
  );
  const found = violations(metadata);
  for (const line of found) console.error(line);
  if (found.length > 0) {
    console.error('Crate dependencies break docs/architecture.md.');
    process.exit(1);
  }
  console.log(`Crate layers checked: ${metadata.packages.length} workspace crates.`);
}
