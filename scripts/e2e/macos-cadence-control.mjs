import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { closeSync, openSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { stopSignalProcesses, waitForSignalBoundary } from './signal-process.mjs';

export function launchCadenceControl(binary, directory, spawnChild = spawn) {
  const file = join(directory, 'cadence-control.jsonl');
  const output = openSync(file, 'wx'), errors = openSync(join(directory, 'cadence-control.stderr.log'), 'wx');
  let child;
  try { child = spawnChild(binary, [], { stdio: ['pipe', output, errors] }); }
  finally { closeSync(output); closeSync(errors); }
  const handle = { child, file, closed: false, error: null, code: null, signal: null };
  child.once('error', error => { handle.error = error; });
  child.stdin.on('error', error => { handle.error = error; });
  child.once('close', (code, signal) => { Object.assign(handle, { closed: true, code, signal }); });
  return handle;
}

function records(handle) {
  return readFileSync(handle.file, 'utf8').split('\n').slice(0, -1).filter(line => line.trim()).map(line => JSON.parse(line));
}

export async function waitCadenceReady(handle, timing = {}) {
  await waitForSignalBoundary(() => {
    assert.ifError(handle.error);
    assert.equal(handle.closed, false, 'The independent cadence client exited before readiness');
    const ready = records(handle).find(item => item.state === 'ready');
    if (!ready) return false;
    assert.equal(ready.bufferFrames, 128, 'The independent aggregate must acknowledge 128 frames');
    assert.equal(ready.rate, 48000);
    assert.equal(ready.inputStreamsEnabled, false, 'The silent client must not request microphone access');
    return true;
  }, 'Independent 128-frame client readiness', { timeoutMs: 30000, ...timing });
}

export async function stopCadenceControl(handle, timing = {}) {
  if (!handle.closed) handle.child.stdin.end();
  try {
    await waitForSignalBoundary(() => handle.closed, 'Independent cadence client exit', { timeoutMs: 5000, ...timing });
  } catch (error) {
    await stopSignalProcesses([handle.child]);
    throw error;
  }
  assert.ifError(handle.error);
  assert.equal(handle.signal, null, 'The cadence client must stop through its own I/O cleanup');
  assert.equal(handle.code, 0, 'The cadence client failed');
  const stopped = records(handle).find(item => item.state === 'stopped');
  assert.ok(stopped, 'Missing independent cadence cleanup report');
  assert.equal(stopped.status, 0);
  return stopped;
}

export function requireCadenceExecution(stopped) {
  assert.ok(stopped.callbacks > 0, 'The independent aggregate never executed a callback');
  assert.equal(stopped.unexpectedFrames, 0, 'The control did not sustain 128-frame callbacks');
}
