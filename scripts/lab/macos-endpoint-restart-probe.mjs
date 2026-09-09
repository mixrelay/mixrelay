// Explicit installed-service acceptance. Saves the live graph before any restart
// and restores it in finally; never restarts coreaudiod or changes default devices.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync, renameSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

assert.equal(process.platform, 'darwin');
const [engine, inspect, evidence, approval] = process.argv.slice(2);
assert.equal(approval, '--restart-installed-service');
assert.ok([engine, inspect, evidence].every(isAbsolute));
const base = 'http://127.0.0.1:19341/api/v1';
const target = 'system/io.tonenari.audio.engine';
const preferences = join(process.env.HOME, 'Library/Application Support/ToneNari/virtual-endpoint.json');
const savedPreferences = existsSync(preferences) ? readFileSync(preferences) : undefined;
let restarts = 0;
function servicePid() {
  const state = execFileSync('/bin/launchctl', ['print', target], { encoding: 'utf8' });
  assert.ok(state.includes(`program = ${engine}\n`), 'Unexpected installed engine executable');
  assert.doesNotMatch(state, /TONENARI_CONFIG_DIR\s*=>/, 'Nonstandard service config; refusing live test');
  const pid = Number(state.match(/\n\s*pid = (\d+)/)?.[1]);
  assert.ok(Number.isSafeInteger(pid) && pid > 1);
  const owner = execFileSync('/bin/ps', ['-p', String(pid), '-o', 'uid='], { encoding: 'utf8' }).trim();
  assert.equal(Number(owner), process.getuid(), 'Refusing to signal another user');
  return pid;
}
async function api(path, body) {
  const response = await fetch(`${base}/${path}`, { method: body ? 'PUT' : 'GET',
    headers: body ? { 'content-type': 'application/json' } : {},
    body: body ? JSON.stringify(body) : undefined, signal: AbortSignal.timeout(3000) });
  const value = await response.json();
  assert.ok(response.ok, JSON.stringify(value));
  return value;
}
async function until(check, message) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    if (await check()) return;
    await delay(100);
  }
  assert.fail(message);
}
async function restart() {
  const pid = servicePid();
  process.kill(pid, 'SIGTERM'); // launchd owns respawn; no global/process-name kill.
  await until(async () => {
    try { return servicePid() !== pid && !!await api('status'); } catch { return false; }
  }, 'Installed service did not restart');
  restarts++;
  const graph = await api('graph');
  await until(async () => (await api('graph/status')).activeRevision === graph.revision, 'Restored graph inactive');
  return graph;
}
async function apply(nodes, edges = []) {
  const graph = await api('graph');
  const revision = graph.revision + 1;
  await api('graph', { ...graph, revision, nodes, edges });
  await until(async () => (await api('graph/status')).activeRevision === revision, 'Graph did not activate');
  assert.equal((await api('status')).audioSession.endpointPreferencesUnavailable, false);
}
function native() {
  const plist = execFileSync(inspect, { timeout: 3000 });
  return JSON.parse(execFileSync('/usr/bin/plutil', ['-convert', 'json', '-o', '-', '-'], { input: plist, encoding: 'utf8' }));
}
async function checkNative(name, render, capture, label) {
  try { await until(() => {
    const result = native();
    return result.displayName === name && result.renderChannels === render && result.captureChannels === capture;
  }, `HAL did not publish ${name} ${render}/${capture}`); }
  finally {
    writeFileSync(join(evidence, `${label}.json`), JSON.stringify({ native: native(), graph: await api('graph'), status: await api('status') }, null, 2), { flag: 'wx' });
  }
}
servicePid();
const original = await api('graph');
const originalNative = native();
// A plugin restart would require instance-state persistence outside this probe.
assert.ok(original.nodes.every(n => n.type === 'virtual_endpoint'), 'Live graph contains other resources; refusing destructive restart');
mkdirSync(evidence, { mode: 0o700 });
writeFileSync(join(evidence, 'original-graph.json'), JSON.stringify(original, null, 2), { flag: 'wx' });
if (savedPreferences) writeFileSync(join(evidence, 'original-preferences.json'), savedPreferences, { flag: 'wx', mode: 0o600 });
try {
  await restart(); // Load the newly built binary before exercising its contract.
  const node = { id: 'default-virtual-io', type: 'virtual_endpoint', label: 'ToneNari 복원 64·16', config: {
    id: 'default-virtual-io', displayName: 'ToneNari 복원 64·16',
    render: { channels: 64, channelLayout: 'discrete' }, capture: { channels: 16, channelLayout: 'discrete' },
    supportedSampleRates: [48000], defaultSampleRate: 48000 } };
  await apply([node]); await checkNative(node.label, 64, 16, 'applied');
  const restored = await restart();
  assert.deepEqual(restored.nodes[0].config, node.config);
  assert.deepEqual(restored.edges, []);
  await checkNative(node.label, 64, 16, 'restored');
  await apply([{ ...node, config: { ...node.config, capture: null } }]);
  const half = await restart();
  assert.equal(half.nodes[0].config.capture, null);
  assert.equal((await api('status')).audioSession.virtualEndpointSettings.defaultEndpoint.spec.capture.channels, 16);
  await apply([]);
  assert.deepEqual((await restart()).nodes, []);
  const template = (await api('status')).audioSession.virtualEndpointSettings.defaultEndpoint;
  await apply([{ id: template.nodeId, type: 'virtual_endpoint', config: template.spec }]);
  await checkNative(node.label, 64, 16, 'readded');
  console.log(`PASS: ${restarts} installed-service restarts; native name/64/16 restore, half/full deletion and recreation`);
} finally {
  await apply(original.nodes, original.edges);
  await checkNative(originalNative.displayName, originalNative.renderChannels, originalNative.captureChannels, 'cleanup');
  if (savedPreferences) {
    const stage = `${preferences}.probe-restore-${process.pid}`;
    writeFileSync(stage, savedPreferences, { flag: 'wx', mode: 0o600 });
    renameSync(stage, preferences);
  }
  console.log('Original graph, native name and channel counts restored. Graph activation is not PCM continuity proof.');
}
