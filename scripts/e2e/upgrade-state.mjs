import assert from 'node:assert/strict';
import { existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, renameSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { digest } from '../support-contract.mjs';

export function upgradeGraph(document, graphId) {
  return {
    ...document,
    revision: document.revision + 1,
    expectedGraphId: graphId,
    nodes: [
      { id: 'upgrade-gain', type: 'gain', config: { channels: 2, gainDb: -6 } },
      { id: 'upgrade-recorder', type: 'recorder', config: { channels: 2 } },
    ],
    edges: [
      {
        id: 'upgrade-routing',
        source: { nodeId: 'upgrade-gain', portId: 'audio' },
        destination: { nodeId: 'upgrade-recorder', portId: 'audio' },
        channelMapping: { mode: 'auto' },
      },
    ],
  };
}

export function preservedState(catalog, driver, path) {
  assert.ok(catalog.document.nodes.length > 0, 'Upgrade needs an authored graph');
  assert.ok(driver.settings.devices.length > 0, 'Upgrade needs saved Driver devices');
  return {
    activeId: catalog.activeId,
    nodes: catalog.document.nodes,
    edges: catalog.document.edges,
    devices: driver.settings.devices.map((device) => device.spec),
    path,
    sha256: digest(readFileSync(path)),
  };
}

export function verifyPreservedState(before, catalog, driver) {
  assert.deepEqual(
    { activeId: catalog.activeId, nodes: catalog.document.nodes, edges: catalog.document.edges },
    { activeId: before.activeId, nodes: before.nodes, edges: before.edges },
    'Upgrade lost the saved graph',
  );
  assert.deepEqual(
    driver.settings.devices.map((device) => device.spec),
    before.devices,
    'Upgrade lost Driver settings',
  );
  assert.equal(
    digest(readFileSync(before.path)),
    before.sha256,
    'Upgrade changed the saved session file',
  );
}

export async function saveWorkspace(api, operation, active, action, path) {
  const current = await api('graphs');
  const reply = await operation(
    'POST',
    '/api/v1/workspace',
    JSON.stringify({
      expectedRevision: current.document.revision,
      expectedCatalogRevision: current.catalogRevision,
      operation: { action, path },
    }),
  );
  assert.equal(reply.status, 200, reply.body);
  await active(reply.json.document.revision);
  return reply.json;
}

export function isolateFreshState(
  os,
  output,
  { home = homedir(), environment = process.env, disposable = false } = {},
) {
  assert.equal(disposable, true, 'State isolation requires a disposable QA session');
  const root = realpathSync(home);
  const config = environment.XDG_CONFIG_HOME || join(root, '.config');
  const data = environment.XDG_DATA_HOME || join(root, '.local/share');
  const paths =
    os === 'macos'
      ? ['MixRelay', 'io.mixrelay.driver', 'io.mixrelay.desktop'].map((name) =>
          join(root, 'Library/Application Support', name),
        )
      : [
          join(config, 'mixrelay'),
          join(config, 'mixrelay-driver'),
          join(config, 'io.mixrelay.desktop'),
          join(data, 'io.mixrelay.desktop'),
        ];
  const normalized = paths.map((path) => resolve(path));
  for (const path of normalized) {
    assert.ok(path.startsWith(`${root}/`), 'QA state must stay inside the session home');
    assert.ok(
      resolve(output) !== path && !resolve(output).startsWith(`${path}/`),
      'Evidence must be outside isolated user state',
    );
    for (let current = path; current !== root; current = dirname(current)) {
      let entry;
      try {
        entry = lstatSync(current);
      } catch (error) {
        if (error.code !== 'ENOENT') throw error;
      }
      assert.ok(!entry?.isSymbolicLink(), `Symlink in QA state path: ${current}`);
    }
  }
  const backup = join(output, 'fresh-state');
  mkdirSync(backup);
  return normalized
    .filter((path) => existsSync(path))
    .map((path, index) => {
      const destination = join(backup, String(index));
      renameSync(path, destination);
      return { path: relative(root, path), backup: relative(output, destination) };
    });
}
