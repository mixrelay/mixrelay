// Read-only installed-engine acceptance. Retains failures and never resets counters.
import { engineSocket, socketJson } from '../local-socket.mjs';
import { open, readFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import assert from 'node:assert/strict';

const [output, logPath, durationText = '600', referencePath] = process.argv.slice(2);
const duration = Number(durationText);
assert(output && logPath && isAbsolute(output) && isAbsolute(logPath), 'Use absolute output and engine log paths');
assert(Number.isInteger(duration) && duration >= 30 && duration <= 3600, 'Duration must be 30..3600 seconds');
assert(!referencePath || isAbsolute(referencePath), 'Reference output must use an absolute path');
const file = await open(output, 'wx');
const log = await open(logPath, 'r');
const buffer = Buffer.alloc(65536);
const failures = [];
const socket = engineSocket();
async function get(path) {
  return socketJson(socket, `/api/v1/${path}`, {
    headers: { host: 'mixrelay-engine' }, timeoutMs: 3000,
  });
}
async function trace() {
  const { size } = await log.stat();
  const { bytesRead } = await log.read(buffer, 0, Math.min(size, buffer.length), Math.max(0, size - buffer.length));
  const rows = buffer.toString('utf8', 0, bytesRead).split('\n');
  const devices = {}, timing = {};
  for (const line of rows.slice(1, -1)) {
    if (!line.includes('native PCM queues') && !line.includes('PCM callback execution timing')) continue;
    const fields = Object.fromEntries([...line.matchAll(/(\w+)=([^\s]+)/g)].map(([, key, value]) => [key,
      value === 'true' ? true : value === 'false' ? false : Number.isFinite(Number(value)) ? Number(value) : value]));
    if (fields.owner === undefined) continue;
    const destination = line.includes('native PCM queues') ? devices : timing;
    destination[fields.owner] = { at: line.split(' ')[0], ...fields };
  }
  return { devices, timing };
}
async function sample() {
  const [status, queues] = await Promise.all([get('status'), trace()]);
  return { at: new Date().toISOString(), status, ...queues };
}
async function emit(row) { await file.write(`${JSON.stringify(row)}\n`); }
let before;
try {
  const graphs = await Promise.all(['graph-1', 'graph-105'].map(id => get(`graphs/${id}`)));
  before = await sample();
  assert(Object.keys(before.devices).length > 0, 'No native device telemetry');
  await emit({ type: 'start', graphs, ...before });
  const started = Date.now();
  let last = before;
  while (Date.now() - started < duration * 1000) {
    await delay(Math.min(5000, duration * 1000 - (Date.now() - started)));
    last = await sample();
    await emit({ type: 'sample', ...last });
    for (const key of ['underruns', 'overruns']) {
      if (last.status[key] !== before.status[key]) failures.push(`${last.at}: ${key} ${before.status[key]} -> ${last.status[key]}`);
    }
    if (last.status.audioSession?.discontinuities !== before.status.audioSession?.discontinuities
      || JSON.stringify(last.status.audioSession?.format) !== JSON.stringify(before.status.audioSession?.format)
      || last.status.audioSession?.issue || last.status.audioSession?.processingState !== 'running') failures.push(`${last.at}: session changed or unhealthy`);
    for (const [id, initial] of Object.entries(before.devices)) {
      const now = last.devices[id];
      if (!now || Date.now() - Date.parse(now.at) > 5000) { failures.push(`${last.at}: device ${id} telemetry missing/stale`); continue; }
      for (const field of ['pcm_starvations', 'overflows', 'clock_gaps', 'epoch', 'native_rate', 'graph_rate', 'enabled', 'ready']) {
        if (initial[field] !== now[field]) failures.push(`${last.at}: device ${id} ${field} ${initial[field]} -> ${now[field]}`);
      }
    }
    console.log(JSON.stringify({ elapsedSeconds: Math.round((Date.now() - started) / 1000), failures: failures.length,
      underruns: last.status.underruns - before.status.underruns, overruns: last.status.overruns - before.status.overruns }));
  }
  const afterGraphs = await Promise.all(['graph-1', 'graph-105'].map(id => get(`graphs/${id}`)));
  if (JSON.stringify(graphs) !== JSON.stringify(afterGraphs)) failures.push('Graph configuration/revision changed');
  // An individually quiet observation window must not erase errors accumulated
  // since the agreed pre-work baseline. Never reset native counters to make it pass.
  if (referencePath) {
    const reference = JSON.parse((await readFile(referencePath, 'utf8')).trim().split('\n').at(-1));
    assert(reference.type === 'result' && reference.after && reference.afterGraphs, 'Reference needs a completed observation');
    if (JSON.stringify(reference.afterGraphs) !== JSON.stringify(afterGraphs)) failures.push('Reference graph configuration/revision changed');
    for (const field of ['underruns', 'overruns']) {
      if (last.status[field] !== reference.after.status[field]) failures.push(`Reference ${field}: ${reference.after.status[field]} -> ${last.status[field]}`);
    }
    for (const field of ['discontinuities', 'format']) {
      if (JSON.stringify(last.status.audioSession?.[field]) !== JSON.stringify(reference.after.status.audioSession?.[field])) failures.push(`Reference session ${field} changed`);
    }
    if (Object.keys(reference.after.devices).sort().join(',') !== Object.keys(last.devices).sort().join(',')) failures.push('Reference native owner set changed');
    for (const [id, initial] of Object.entries(reference.after.devices)) {
      for (const field of ['pcm_starvations', 'overflows', 'clock_gaps', 'epoch', 'native_rate', 'graph_rate', 'enabled', 'ready']) {
        if (initial[field] !== last.devices[id]?.[field]) failures.push(`Reference device ${id} ${field}: ${initial[field]} -> ${last.devices[id]?.[field]}`);
      }
    }
  }
  await emit({ type: 'result', referencePath, durationSeconds: (Date.now() - started) / 1000, passed: failures.length === 0, failures,
    before, after: last, afterGraphs });
  process.exitCode = failures.length ? 1 : 0;
} catch (error) {
  await emit({ type: 'error', at: new Date().toISOString(), error: String(error) });
  throw error;
} finally {
  await log.close();
  await file.close();
}
