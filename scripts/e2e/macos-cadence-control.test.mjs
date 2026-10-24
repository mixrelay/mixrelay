import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { appendFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { launchCadenceControl, requireCadenceExecution, stopCadenceControl, waitCadenceReady } from './macos-cadence-control.mjs';

function fixture(t, { ready = { state: 'ready', bufferFrames: 128, rate: 48000, inputStreamsEnabled: false }, stopped = {} } = {}) {
  const directory = mkdtempSync(join(tmpdir(), 'mr-cadence-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const file = join(directory, 'cadence-control.jsonl');
  const child = new EventEmitter();
  child.stdin = new EventEmitter();
  child.exitCode = null;
  child.signalCode = null;
  let ended = 0;
  child.stdin.end = () => {
    ended++;
    appendFileSync(file, JSON.stringify({ state: 'stopped', status: 0, callbacks: 412,
      clockBreaks: 1, unexpectedFrames: 0, ...stopped }) + '\n');
    child.exitCode = 0;
    child.emit('close', 0, null);
  };
  child.kill = () => assert.fail('Normal EOF must drain the native client without a signal');
  const handle = launchCadenceControl('/owned/control', directory, (_binary, _args, options) => {
    assert.equal(options.stdio[0], 'pipe');
    if (ready) writeFileSync(file, JSON.stringify(ready) + '\n');
    return child;
  });
  return { handle, child, file, ended: () => ended };
}

test('the separate client acknowledges its cadence and drains I/O before comparison cleanup', async t => {
  const f = fixture(t);
  await waitCadenceReady(f.handle);
  assert.deepEqual(await stopCadenceControl(f.handle), { state: 'stopped', status: 0,
    callbacks: 412, clockBreaks: 1, unexpectedFrames: 0 });
  assert.equal(f.ended(), 1);
  assert.equal(f.handle.closed, true);
});

test('readiness waits for a complete record, and rejects a different acknowledged cadence', async t => {
  const f = fixture(t, { ready: null });
  writeFileSync(f.file, '{"state":"ready",');
  let now = 0;
  await waitCadenceReady(f.handle, { now: () => now, wait: async () => {
    now += 50;
    appendFileSync(f.file, '"bufferFrames":512,"rate":48000,"inputStreamsEnabled":false}\n');
  } }).then(() => assert.fail('A 512-frame companion cannot establish the engine deadline'),
    error => assert.match(error.message, /acknowledge 128/));
  await stopCadenceControl(f.handle);
});

test('an exited client cannot establish readiness and invalid execution cannot pass', async t => {
  const f = fixture(t, { stopped: { unexpectedFrames: 4 } });
  const stopped = await stopCadenceControl(f.handle);
  assert.throws(() => requireCadenceExecution(stopped), /sustain 128/);
  assert.throws(() => requireCadenceExecution({ callbacks: 0, unexpectedFrames: 0 }), /never executed/);
  await assert.rejects(waitCadenceReady(f.handle), /exited before readiness/);
  assert.equal(f.ended(), 1);
  assert.equal(f.handle.closed, true);
});
