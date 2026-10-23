import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { preservedState, saveWorkspace, verifyPreservedState } from './upgrade-state.mjs';

function fixture(t) {
  const directory = mkdtempSync(join(tmpdir(), 'mixrelay-upgrade-state-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const path = join(directory, 'session.mix');
  writeFileSync(path, 'real saved document fixture');
  const catalog = {
    activeId: 'saved-graph',
    document: {
      revision: 7,
      nodes: [{ id: 'user-node', type: 'recorder', config: { channels: 2 } }],
      edges: [],
    },
  };
  const driver = {
    settings: {
      devices: [
        {
          spec: { id: 'saved-device', displayName: 'MixRelay IO' },
          nativeDeviceIds: ['old-native-id'],
        },
      ],
    },
  };
  return { path, catalog, driver };
}

test('preservation compares authored content, stable device settings and the saved file across runtime revisions', (t) => {
  const f = fixture(t),
    before = preservedState(f.catalog, f.driver, f.path);
  f.catalog.document.revision = 22;
  f.driver.settings.devices[0].nativeDeviceIds = ['new-native-id'];
  verifyPreservedState(before, f.catalog, f.driver);
  for (const mutate of [
    (value) => {
      value.catalog.activeId = 'other';
    },
    (value) => {
      value.catalog.document.nodes = [];
    },
    (value) => {
      value.catalog.document.edges = [{ id: 'lost-route' }];
    },
    (value) => {
      value.driver.settings.devices = [];
    },
  ]) {
    const changed = structuredClone(f);
    mutate(changed);
    assert.throws(() => verifyPreservedState(before, changed.catalog, changed.driver));
  }
  writeFileSync(f.path, 'overwritten');
  assert.throws(() => verifyPreservedState(before, f.catalog, f.driver), /saved session file/);
});

test('an empty workspace cannot count as upgrade preservation evidence', (t) => {
  const f = fixture(t);
  f.catalog.document.nodes = [];
  assert.throws(() => preservedState(f.catalog, f.driver, f.path), /authored graph/);
});

test('workspace save/open supplies current concurrency revisions and waits for the activated reply', async () => {
  const activated = [],
    operations = [];
  const api = async (route) => {
    assert.equal(route, 'graphs');
    return { document: { revision: 9 }, catalogRevision: 12 };
  };
  const operation = async (...args) => {
    operations.push(args);
    return { status: 200, json: { document: { revision: 10 } } };
  };
  await saveWorkspace(
    api,
    operation,
    async (revision) => activated.push(revision),
    'save',
    '/session.mix',
  );
  assert.deepEqual(operations, [
    [
      'POST',
      '/api/v1/workspace',
      JSON.stringify({
        expectedRevision: 9,
        expectedCatalogRevision: 12,
        operation: { action: 'save', path: '/session.mix' },
      }),
    ],
  ]);
  assert.deepEqual(activated, [10]);
  await assert.rejects(
    saveWorkspace(
      api,
      async () => ({ status: 409, body: 'conflict' }),
      async () => assert.fail('must not activate'),
      'open',
      '/session.mix',
    ),
    /conflict/,
  );
});

test('fresh state is archived before baseline initialization while external documents stay in place', async (t) => {
  const { existsSync, mkdirSync, readFileSync, realpathSync } = await import('node:fs');
  const { isolateFreshState } = await import('./upgrade-state.mjs');
  for (const os of ['macos', 'linux']) {
    const root = mkdtempSync(join(tmpdir(), 'mixrelay-isolate-state-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    const home = realpathSync(root),
      output = join(home, 'evidence');
    mkdirSync(output);
    const directories =
      os === 'macos'
        ? [
            'Library/Application Support/MixRelay',
            'Library/Application Support/io.mixrelay.driver',
            'Library/Application Support/io.mixrelay.desktop',
          ]
        : [
            '.config/mixrelay',
            '.config/mixrelay-driver',
            '.config/io.mixrelay.desktop',
            '.local/share/io.mixrelay.desktop',
          ];
    for (const path of directories) {
      mkdirSync(join(home, path), { recursive: true });
      writeFileSync(join(home, path, 'state.json'), 'fresh candidate state');
    }
    writeFileSync(join(home, 'project.mix'), 'saved user document');
    const archived = isolateFreshState(os, output, { home, environment: {}, disposable: true });
    assert.equal(archived.length, directories.length);
    for (const item of archived) {
      assert.equal(existsSync(join(home, item.path)), false);
      assert.equal(
        readFileSync(join(output, item.backup, 'state.json'), 'utf8'),
        'fresh candidate state',
      );
    }
    assert.equal(readFileSync(join(home, 'project.mix'), 'utf8'), 'saved user document');
  }
});

test('state isolation rejects unacknowledged hosts, external paths and live or dangling symlinks before moving anything', async (t) => {
  const { mkdirSync, realpathSync, symlinkSync, existsSync } = await import('node:fs');
  const { isolateFreshState } = await import('./upgrade-state.mjs');
  const root = mkdtempSync(join(tmpdir(), 'mixrelay-isolate-guard-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const home = realpathSync(root),
    output = join(home, 'evidence');
  mkdirSync(output);
  const state = join(home, '.config/mixrelay');
  mkdirSync(state, { recursive: true });
  writeFileSync(join(state, 'graphs.json'), 'keep');
  assert.throws(() => isolateFreshState('linux', output, { home, environment: {} }), /disposable/);
  assert.throws(
    () =>
      isolateFreshState('linux', output, {
        home,
        environment: { XDG_CONFIG_HOME: '/etc' },
        disposable: true,
      }),
    /inside the session home/,
  );
  symlinkSync(join(home, 'missing-target'), join(home, '.config/mixrelay-driver'));
  assert.throws(
    () => isolateFreshState('linux', output, { home, environment: {}, disposable: true }),
    /Symlink/,
  );
  assert.equal(existsSync(join(state, 'graphs.json')), true);
  assert.equal(existsSync(join(output, 'fresh-state')), false);
});

test('baseline state contains a nondefault processor parameter and routing without needing a native audio source', async () => {
  const { upgradeGraph } = await import('./upgrade-state.mjs');
  const original = { schemaVersion: 2, revision: 8, nodes: [], edges: [] };
  const document = upgradeGraph(original, 'baseline-graph');
  assert.equal(document.expectedGraphId, 'baseline-graph');
  assert.equal(document.revision, 9);
  assert.deepEqual(original.nodes, []);
  assert.deepEqual(
    document.nodes.map((node) => node.type),
    ['gain', 'recorder'],
  );
  assert.equal(document.nodes[0].config.gainDb, -6);
  assert.equal(document.nodes[0].config.channels, document.nodes[1].config.channels);
  assert.deepEqual(document.edges[0].source, { nodeId: document.nodes[0].id, portId: 'audio' });
  assert.deepEqual(document.edges[0].destination, {
    nodeId: document.nodes[1].id,
    portId: 'audio',
  });
});
