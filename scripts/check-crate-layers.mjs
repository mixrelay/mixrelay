#!/usr/bin/env node
// Checks that workspace crates depend only in the directions docs/architecture.md allows.
// Reads `cargo metadata`; prints each forbidden dependency and exits 1 when any exist.
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

/** Layer of each workspace crate. A crate may depend on lower layers and on the crates
 * `SAME_LAYER` allows; apps depend on anything but apps; test support only as dev. */
export const LAYERS = {
  'mixrelay-audio-io': 'foundation',
  'mixrelay-pcm-ring': 'foundation',
  'mixrelay-realtime': 'foundation',
  'mixrelay-local-socket': 'foundation',
  'mixrelay-licensing': 'foundation',
  'mixrelay-processor-state': 'foundation',
  'mixrelay-engine-process': 'foundation',
  'mixrelay-audio-graph': 'model',
  'mixrelay-audio-recorder': 'model',
  'mixrelay-audio-engine': 'model',
  'mixrelay-control-protocol': 'contract',
  'mixrelay-driver-control': 'contract',
  'mixrelay-control-server': 'service',
  'mixrelay-processor-host': 'service',
  'mixrelay-macos-transport': 'macos',
  'mixrelay-hal-core': 'macos',
  'mixrelay-process-tap': 'macos',
  'mixrelay-macos-driver': 'macos',
  'mixrelay-macos-installer': 'macos',
  'mixrelay-hal-bundle': 'macos',
  'mixrelay-pipewire': 'linux',
  'mixrelay-linux-driver': 'linux',
  'mixrelay-engine-service': 'app',
  'mixrelay-driver-service': 'app',
  'mixrelay-desktop': 'app',
  'mixrelay-test-support': 'test',
  'mixrelay-macos-test-support': 'test',
  'mixrelay-native-clap-fixture': 'test',
  'mixrelay-pipewire-observer': 'test',
  'truce-rack-core': 'vendor',
  'truce-rack-clap': 'vendor',
  'truce-rack-vst3': 'vendor',
};

const RANK = { vendor: 0, foundation: 0, model: 1, contract: 2, service: 3 };
const PLATFORM = new Set(['macos', 'linux']);

/** Same-layer or upward dependencies that are part of the design. */
const SAME_LAYER = new Set([
  'mixrelay-audio-engine -> mixrelay-audio-graph',
  'mixrelay-audio-engine -> mixrelay-audio-recorder',
  'mixrelay-hal-core -> mixrelay-macos-transport',
  'mixrelay-process-tap -> mixrelay-macos-transport',
  'mixrelay-macos-driver -> mixrelay-macos-transport',
  'mixrelay-linux-driver -> mixrelay-pipewire',
  'truce-rack-clap -> truce-rack-core',
  'truce-rack-vst3 -> truce-rack-core',
  // Known exception: plugin descriptors and editor frames are defined in the protocol.
  'mixrelay-processor-host -> mixrelay-control-protocol',
]);

/** Crates that host plugins and may depend on the vendored plugin formats. */
const PLUGIN_HOSTS = new Set(['mixrelay-processor-host']);

/** Why `from -> to` is forbidden, or undefined when allowed. Dev dependencies follow the
 * same table and may additionally use test support. */
export function violation(from, to, kind) {
  const source = LAYERS[from];
  const target = LAYERS[to];
  if (source === undefined) return `${from} has no layer in scripts/check-crate-layers.mjs`;
  if (target === undefined) return `${to} has no layer in scripts/check-crate-layers.mjs`;
  if (target === 'test') return kind === 'dev' ? undefined : 'test support is a dev-dependency only';
  if (source === 'test') return target === 'app' ? 'nothing depends on an app' : undefined;
  if (SAME_LAYER.has(`${from} -> ${to}`)) return undefined;
  if (target === 'app') return 'nothing depends on an app';
  if (source === 'app') return undefined;
  if (PLATFORM.has(target)) {
    return PLATFORM.has(source) ? 'one platform adapter depends only on its own listed adapters' : 'platform-neutral crates never depend on a platform adapter';
  }
  if (target === 'vendor') {
    return source === 'vendor' || PLUGIN_HOSTS.has(from) ? undefined : 'only the plugin host depends on vendored plugin formats';
  }
  // Adapters build on foundation primitives and on contracts, never on models or services.
  if (PLATFORM.has(source)) return ['foundation', 'contract'].includes(target) ? undefined : `${source} adapters may not depend on ${target}`;
  return RANK[target] < RANK[source] ? undefined : `${source} may not depend on ${target}`;
}

/** Forbidden dependencies among workspace packages in `cargo metadata` output. */
export function violations(metadata) {
  const members = new Set(metadata.packages.map((item) => item.name));
  const found = [];
  for (const item of metadata.packages) {
    for (const dependency of item.dependencies) {
      if (!members.has(dependency.name)) continue;
      const kind = dependency.kind === 'dev' ? 'dev' : 'normal';
      const reason = violation(item.name, dependency.name, kind);
      if (reason) found.push(`${item.name} -> ${dependency.name} (${kind}): ${reason}`);
    }
  }
  return [...new Set(found)].sort();
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
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
