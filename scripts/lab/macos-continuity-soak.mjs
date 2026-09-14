// Read-only installed-engine acceptance. Retains failures and never resets counters.
import { open } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import assert from 'node:assert/strict';

const [output, logPath, durationText = '600'] = process.argv.slice(2);
const duration = Number(durationText);
assert(output && logPath && isAbsolute(output) && isAbsolute(logPath), 'Use absolute output and engine log paths');
assert(Number.isInteger(duration) && duration >= 30 && duration <= 3600, 'Duration must be 30..3600 seconds');
const file = await open(output, 'wx');
const log = await open(logPath, 'r');
const buffer = Buffer.alloc(65536);
const failures = [];
async function get(path) {
  const response = await fetch(`http://127.0.0.1:19341/api/v1/${path}`, { signal: AbortSignal.timeout(3000) });
  assert(response.ok, `${path}: ${response.status}`);
  return response.json();
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
  await emit({ type: 'result', durationSeconds: (Date.now() - started) / 1000, passed: failures.length === 0, failures,
    before, after: last, afterGraphs });
  process.exitCode = failures.length ? 1 : 0;
} catch (error) {
  await emit({ type: 'error', at: new Date().toISOString(), error: String(error) });
  throw error;
} finally {
  await log.close();
  await file.close();
}
