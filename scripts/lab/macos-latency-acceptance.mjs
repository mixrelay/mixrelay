// Installed acceptance for recording-device latency reporting (docs/design/audio-content-latency.md).
// Adds one route SOURCE device input -> engine -> virtual endpoint to the selected graph, waits
// for the HAL to report a stable input latency on that endpoint, optionally measures the real
// round trip, then restores the exact original graph and checks that the report clears.
//   node scripts/lab/macos-latency-acceptance.mjs SOURCE_UID ENDPOINT_ID EVIDENCE_DIR --verify-installed-latency [--roundtrip]
// SOURCE_UID must be a loopback device (for example Loopback Audio) whose output returns on its
// input, so the probe can play into it while recording only from the endpoint. The probe never
// plays into the endpoint itself: that would bypass the engine and gate the report to zero.
// --roundtrip records audio, so the terminal needs microphone permission.
// The endpoint must be unused by every saved graph, so the test never adds a contributor to a
// live route. Writes name the selected graph (expectedGraphId) and its revision, so another
// client's edit or graph selection makes them fail instead of overwriting that graph.
import { engineSocket, socketRequest } from '../local-socket.mjs';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const [sourceUid, endpointId, evidence, approval, ...options] = process.argv.slice(2);
assert.equal(process.platform, 'darwin');
assert.equal(approval, '--verify-installed-latency', 'Pass --verify-installed-latency to change the installed graph');
assert.ok(options.every(option => option === '--roundtrip'), `Unknown option: ${options.join(' ')}`);
assert.ok(sourceUid && !sourceUid.startsWith('io.mixrelay.'), 'The source must be another vendor\'s loopback device');
assert.ok(evidence && isAbsolute(evidence), 'Evidence directory must be an absolute path');
assert.match(endpointId ?? '', /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/);
const roundtrip = options.includes('--roundtrip');
const uid = `io.mixrelay.audio.endpoint.${endpointId.replaceAll('-', '')}`;
const probe = fileURLToPath(new URL('../probe-macos-roundtrip.sh', import.meta.url));
const socket = engineSocket();
const ids = { source: 'latency-acceptance-source', destination: 'latency-acceptance-destination' };
// The reporter publishes once a second; the HAL needs three stable windows.
const PUBLICATION_INTERVAL_MS = 1100;

// Engine requests run as operations (crates/control-server/src/operations.rs). A client
// operation ID makes a request idempotent while its receipt is retained (60 s), so a lost
// reply is resent and a pending receipt is polled until the terminal result. The budget stays
// below that retention, so a resend can never execute the request a second time.
const OPERATION_BUDGET_MS = 45000;
const engineHost = { host: 'mixrelay-engine' };
async function operation(method, path, body) {
  const id = `latency-acceptance-${randomUUID()}`;
  const receipt = `/api/v1/operations/${id}`;
  const headers = { ...engineHost, 'x-mixrelay-operation-request': id,
    ...(body === undefined ? {} : { 'content-type': 'application/json' }) };
  const deadline = Date.now() + OPERATION_BUDGET_MS;
  let pending = false;
  while (Date.now() < deadline) {
    let reply;
    try {
      reply = pending
        ? await socketRequest(socket, receipt, { headers: engineHost, timeoutMs: 5000 })
        : await socketRequest(socket, path, { method, headers, body, timeoutMs: 5000 });
    } catch {
      await delay(250); // lost reply: resend the identical request or poll again
      continue;
    }
    if (reply.headers['x-mixrelay-operation'] !== undefined) {
      pending = true;
      await delay(250);
      continue;
    }
    // Routes outside the operation layer answer directly; an expired receipt or a lost task
    // answers "uncertain", which never confirms whether the request took effect.
    let parsed;
    try {
      parsed = reply.body ? JSON.parse(reply.body) : undefined;
    } catch {
      parsed = undefined;
    }
    assert.notEqual(parsed?.parameters?.operationOutcome, 'uncertain',
      `${method} ${path} has no confirmed result (operation ${id}); check the graph before editing it`);
    if (reply.headers['x-mixrelay-operation-result'] === id) {
      // Acknowledging only reclaims the receipt; the result above is already final.
      await socketRequest(socket, receipt, { method: 'DELETE', headers: engineHost, timeoutMs: 5000 })
        .catch(() => {});
    }
    return { ...reply, json: parsed };
  }
  assert.fail(`${method} ${path} did not finish (operation ${id}); check the graph before editing it`);
}
async function api(path, body) {
  const reply = await operation(body === undefined ? 'GET' : 'PUT', `/api/v1/${path}`,
    body === undefined ? undefined : JSON.stringify(body));
  assert.ok(reply.status >= 200 && reply.status < 300, `/api/v1/${path}: HTTP ${reply.status} ${reply.body}`);
  return reply.json;
}
function reported() {
  const text = execFileSync('zsh', [probe, '--latency', uid], { encoding: 'utf8', timeout: 10000 });
  const input = text.split('\n').find(line => line.startsWith('input '));
  const match = input?.match(/reports latency (\d+) \+ safety (\d+) \+ stream (\d+) frames/);
  assert.ok(match, `Cannot read the reported input latency:\n${text}`);
  return { latency: Number(match[1]), safety: Number(match[2]), stream: Number(match[3]) };
}
async function until(what, check, attempts, intervalMs = 100) {
  for (let attempt = 0; attempt < attempts; attempt++) {
    const value = await check();
    if (value) return value;
    await delay(intervalMs);
  }
  assert.fail(`Timed out waiting for ${what}`);
}
// Applies a graph only while `expectedGraphId` is still selected; returns the terminal reply.
const applyGraph = (document, expectedGraphId = graphId) =>
  operation('PUT', '/api/v1/graph', JSON.stringify({ ...document, expectedGraphId }));
const ours = key => Object.values(ids).some(id => key === id || key.startsWith(`${id}::`));

// Preconditions: nothing else routes to this endpoint or reports a latency for it.
const catalog = await api('graphs');
const graphId = catalog.activeId;
for (const { id } of catalog.graphs) {
  const document = id === graphId ? catalog.document : await api(`graphs/${encodeURIComponent(id)}`);
  assert.ok(!document.nodes.some(node => node.type === 'virtual_endpoint' && node.config.id === endpointId),
    `Graph ${id} already uses this endpoint; refusing to add a contributor to a live route`);
}
const device = (await api('status')).audioSession.virtualEndpointSettings.devices
  .find(candidate => candidate.spec.id === endpointId);
assert.ok(device?.nativeDeviceIds.includes(uid), 'Endpoint is not published by the current engine');
assert.ok(device.spec.capture, 'The endpoint needs a recording side');
const original = await api('graph');
assert.deepEqual(original, catalog.document, 'Graph changed during preparation');
assert.ok(!original.nodes.some(node => Object.values(ids).includes(node.id)));
// Older engines ignore expectedGraphId, which would leave writes unguarded. A stale revision
// with an unselected graph ID never applies: guarded engines name the selected graph instead.
const guard = await applyGraph(original, `${ids.source}-guard-check`);
assert.equal(guard.status, 409, `Unexpected guard check reply: HTTP ${guard.status} ${guard.body}`);
assert.equal(guard.json?.parameters?.selectedGraphId, graphId,
  'This engine does not support expectedGraphId; update MixRelay before running this test');
assert.deepEqual(await api('graph'), original, 'Graph changed during the guard check');
const before = reported();
assert.equal(before.latency, 0, 'The endpoint already reports a latency; refusing to judge a stale value');
mkdirSync(evidence, { mode: 0o700 });
const record = (name, value) =>
  writeFileSync(join(evidence, name), `${JSON.stringify(value, null, 2)}\n`, { flag: 'wx' });
record('before.json', { graphId, graph: original, reported: before });

const test = structuredClone(original);
test.revision++;
test.nodes.push(
  { id: ids.source, label: 'Latency source', type: 'audio_device_input',
    config: { selector: { match: 'persistent_uid', uid: sourceUid }, outputChannels: device.spec.capture.channels } },
  { id: ids.destination, label: 'Latency destination', type: 'virtual_endpoint',
    config: { ...device.spec, reference: { render: false, capture: true } } },
);
test.edges.push({
  id: 'latency-acceptance-route',
  source: { nodeId: ids.source, portId: 'audio' },
  destination: { nodeId: ids.destination, portId: 'to_os' },
  channelMapping: { mode: 'auto' },
});
// 'unknown' while a write may still commit; cleanup never guesses in that state. Only a
// confirmed 'applied' write is ours to restore: a rejected one may have lost to another writer.
let mutation = 'none';
let applied;
const result = { graphId, source: sourceUid, endpoint: uid };
const failures = [];
try {
  mutation = 'unknown';
  const applying = await applyGraph(test);
  mutation = applying.status >= 200 && applying.status < 300 ? 'applied' : 'rejected';
  assert.ok(applying.status >= 200 && applying.status < 300, `Test graph rejected: HTTP ${applying.status} ${applying.body}`);
  await until('the test graph', async () => (await api('graph/status')).activeRevision === test.revision, 100);
  // The engine normalizes what it stores (for example default node controls), so the
  // restore check compares against its applied document rather than the submitted one.
  applied = await api('graph');
  assert.equal(applied.revision, test.revision);
  assert.deepEqual(applied.edges, test.edges);
  // Samples about one publication apart, so three equal values span three reporter windows.
  const readings = (result.readings = []);
  result.reported = await until('a stable reported input latency', () => {
    readings.push(reported());
    const last = readings.slice(-3);
    return last.length === 3 && last[0].latency > 0 && last.every(value => value.latency === last[0].latency)
      ? last[0] : undefined;
  }, 30, PUBLICATION_INTERVAL_MS);
  if (roundtrip) {
    const output = execFileSync('zsh', [probe, '--json', '--seconds', '5', sourceUid, uid], { encoding: 'utf8', timeout: 60000 });
    result.roundtrip = JSON.parse(output);
  }
  record('measured.json', { graph: applied, ...result });
} catch (error) {
  failures.push(error);
}
// Cleanup runs whether or not the measurement succeeded; its own failure is kept too.
try {
  if (mutation === 'unknown') {
    assert.fail(`The test graph operation has no terminal result; check graph ${graphId} (original saved in ${evidence})`);
  }
  if (mutation === 'applied') await restore();
} catch (error) {
  failures.push(error);
}
if (failures.length) {
  // Keep what was observed so a failed run can be diagnosed from its evidence.
  record('failed.json', {
    errors: failures.map(String), graph: applied, status: await api('status').catch(String), ...result,
  });
  throw failures[0];
}

async function restore() {
  // Our write was confirmed at test.revision and every write advances the revision.
  const current = await api('graph');
  // Every edit advances the revision, so an unchanged revision and document mean no one else edited.
  const untouched = current.revision === test.revision &&
    (applied ? JSON.stringify(current) === JSON.stringify(applied)
      : JSON.stringify(current.edges) === JSON.stringify(test.edges));
  assert.ok(untouched, `Concurrent edit: refusing automatic restore; original saved in ${evidence}`);
  const restored = { ...original, revision: test.revision + 1 };
  const restoring = await applyGraph(restored);
  assert.ok(restoring.status >= 200 && restoring.status < 300,
    `Restore rejected: HTTP ${restoring.status} ${restoring.body}; original saved in ${evidence}`);
  await until('the restored graph', async () => (await api('graph/status')).activeRevision === restored.revision, 100);
  assert.deepEqual(await api('graph'), restored);
  // An open editor saves positions for the temporary nodes; drop only those.
  const layout = await api(`graphs/${encodeURIComponent(graphId)}/layout`);
  const positions = Object.fromEntries(Object.entries(layout.layout.positions).filter(([key]) => !ours(key)));
  if (Object.keys(positions).length !== Object.keys(layout.layout.positions).length) {
    await api(`graphs/${encodeURIComponent(graphId)}/layout`, {
      expectedLayoutRevision: layout.layoutRevision, layout: { ...layout.layout, positions },
    });
  }
  // With the route gone the endpoint has no measurement, so the HAL must report unknown (0).
  record('restored.json', { graph: restored, reported: await until('the reported latency to clear', () => {
    const value = reported();
    return value.latency === 0 ? value : undefined;
  }, 10, PUBLICATION_INTERVAL_MS) });
}

console.log(JSON.stringify({
  endpoint: uid,
  source: sourceUid,
  reportedInputLatencyFrames: result.reported.latency,
  roundtripLatencyFrames: result.roundtrip?.latencyFrames,
  roundtripLatencyMs: result.roundtrip?.latencyMs,
  evidence,
}, null, 2));
