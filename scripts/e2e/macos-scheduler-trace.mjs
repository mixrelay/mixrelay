import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { stopSignalProcesses, waitForSignalBoundary } from './signal-process.mjs';

// Record the running system independently of LaunchServices: profiling `open`
// alone can end before the launched application starts its measurement.
export async function withMacosSchedulerTrace(directory, measure, {
  start = spawn, wait = waitForSignalBoundary,
} = {}) {
  assert.equal(process.env.GITHUB_ACTIONS, 'true', 'Scheduler diagnosis requires a disposable hosted Mac');
  const child = start('xcrun', ['xctrace', 'record', '--template', 'System Trace', '--no-prompt',
    '--all-processes', '--output', join(directory, 'scheduler.trace'), '--time-limit', '45s', '--window', '45s'],
  { stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '', closed = false, failure;
  const errors = [];
  child.on('error', error => { failure = error; });
  child.once('close', (code, signal) => {
    closed = true;
    if (code !== 0) failure = new Error(`System Trace exited with code ${code}, signal ${signal}`);
  });
  const observe = bytes => {
    if (log.length + bytes.length > 1024 * 1024) {
      failure = new Error('System Trace exceeded its bounded diagnostic output');
      child.kill('SIGINT');
    } else log += bytes.toString();
  };
  child.stdout.on('data', observe);
  child.stderr.on('data', observe);
  let result;
  try {
    await wait(() => {
      assert.ifError(failure);
      assert.equal(closed, false, 'System Trace exited before measurement');
      return log.includes('Recording started') || log.includes('Ctrl-C to stop the recording');
    }, 'System Trace readiness', { timeoutMs: 30000 });
    result = await measure();
    assert.equal(closed, false, 'System Trace ended before measurement completed');
  } catch (error) { errors.push(error); }
  try {
    if (!closed) child.kill('SIGINT');
    await wait(() => closed, 'System Trace finalization', { timeoutMs: 90000 });
    assert.ifError(failure);
  } catch (error) {
    errors.push(error);
    try { await stopSignalProcesses([child]); } catch (stopError) { errors.push(stopError); }
  } finally {
    writeFileSync(join(directory, 'scheduler.log'), log, { flag: 'wx' });
  }
  if (errors.length) throw new AggregateError(errors, errors.map(error => error.message).join('; '));
  return result;
}
