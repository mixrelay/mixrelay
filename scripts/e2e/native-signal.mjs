import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { rawPcmArgs } from '../lab/pipewire-cli.mjs';
import { collectMacosLoad, snapshotMacosProcesses } from './collect-macos.mjs';

export function analyseStereo(bytes) {
  assert.equal(bytes.length % 8, 0, 'Incomplete stereo Float32 frames');
  const peaks = [0, 0];
  let active = 0;
  for (let offset = 0; offset < bytes.length; offset += 8) {
    let audible = false;
    for (let channel = 0; channel < 2; channel++) {
      const value = bytes.readFloatLE(offset + channel * 4);
      assert.ok(Number.isFinite(value), 'Non-finite recorded sample');
      peaks[channel] = Math.max(peaks[channel], Math.abs(value));
      audible ||= Math.abs(value) > 0.0001;
    }
    if (audible) active++;
  }
  assert.ok(active > 24000, 'Less than half a second of native stereo signal');
  assert.ok(peaks.every(peak => peak > 0.001 && peak <= 1), 'Missing or clipped channel');
  return { frames: bytes.length / 8, active, peaks };
}

export function analyseRf64(bytes) {
  assert.equal(bytes.toString('ascii', 0, 4), 'RF64');
  assert.equal(bytes.toString('ascii', 8, 12), 'WAVE');
  assert.equal(bytes.readUInt32LE(4), 0xffffffff);
  let offset = 12, format, data, sizes;
  while (offset + 8 <= bytes.length) {
    const kind = bytes.toString('ascii', offset, offset + 4);
    const size = bytes.readUInt32LE(offset + 4);
    if (kind === 'ds64') {
      assert.equal(size, 28);
      sizes = { riff: bytes.readBigUInt64LE(offset + 8), data: bytes.readBigUInt64LE(offset + 16), frames: bytes.readBigUInt64LE(offset + 24) };
    }
    if (kind === 'fmt ') {
      format = { encoding: bytes.readUInt16LE(offset + 8), channels: bytes.readUInt16LE(offset + 10),
        rate: bytes.readUInt32LE(offset + 12), bits: bytes.readUInt16LE(offset + 22) };
      if (format.encoding === 0xfffe) {
        assert.equal(size, 40);
        assert.equal(bytes.readUInt16LE(offset + 24), 22);
        assert.equal(bytes.readUInt16LE(offset + 26), 32);
        assert.equal(bytes.subarray(offset + 32, offset + 48).toString('hex'), '0300000000001000800000aa00389b71', 'Float32 subformat required');
        format.encoding = 3;
      }
    }
    if (kind === 'data') { assert.equal(size, 0xffffffff); data = bytes.subarray(offset + 8); break; }
    offset += 8 + size + (size % 2);
  }
  assert.deepEqual(format, { encoding: 3, channels: 2, rate: 48000, bits: 32 });
  assert.ok(data, 'Missing finalized recording data');
  assert.deepEqual(sizes, { riff: BigInt(bytes.length - 8), data: BigInt(data.length), frames: BigInt(data.length / 8) }, 'Unfinalized RF64 sizes');
  return { format, ...analyseStereo(data) };
}

export async function stopSignalProcesses(children, graceMs = 5000) {
  const live = child => child.exitCode === null && child.signalCode === null;
  const wait = child => new Promise(resolve => {
    if (!live(child)) { resolve(); return; }
    const done = () => { clearTimeout(timer); child.removeListener('exit', done); resolve(); };
    const timer = setTimeout(done, graceMs);
    child.once('exit', done);
  });
  const failures = [];
  for (const child of children.filter(Boolean)) {
    if (live(child)) child.kill('SIGTERM');
    await wait(child);
    if (live(child)) {
      child.kill('SIGKILL');
      await wait(child);
      failures.push(live(child) ? 'Native signal process did not stop after SIGKILL' : 'Native signal process required SIGKILL');
    }
  }
  if (failures.length) throw new Error(failures.join('; '));
}

export async function waitForSignalBoundary(read, description, {
  timeoutMs = 30000, now = Date.now, wait = delay,
} = {}) {
  const deadline = now() + timeoutMs;
  for (;;) {
    assert.ok(now() < deadline, `${description} did not complete before its deadline`);
    if (read()) return;
    await wait(50);
  }
}

async function linuxSignal(device, directory) {
  const render = device.nativeDeviceIds.find(id => id.endsWith('.render'));
  const capture = device.nativeDeviceIds.find(id => id.endsWith('.capture'));
  assert.ok(render && capture, 'Missing exact native endpoint identities');
  const help = execFileSync('pw-cat', ['--help'], { encoding: 'utf8', timeout: 5000 });
  const args = rawPcmArgs(help);
  const recorder = spawn('pw-cat', ['--record', ...args, '--target', capture, '-'], { stdio: ['ignore', 'pipe', 'pipe'] });
  const chunks = [];
  let size = 0, error;
  recorder.on('error', value => { error = value; });
  recorder.stdout.on('data', bytes => { size += bytes.length; if (size > 8 * 1024 * 1024) { error = new Error('Native evidence exceeded its bound'); recorder.kill('SIGTERM'); } else chunks.push(bytes); });
  recorder.stderr.resume();
  let recorderClosed = false;
  recorder.once('close', () => { recorderClosed = true; });
  let player;
  let playerClosed = false;
  const timeoutMs = process.env.MIXRELAY_E2E_VM_ACCELERATOR === 'tcg' ? 120000 : 30000;
  try {
    await waitForSignalBoundary(() => {
      assert.ifError(error);
      assert.ok(!recorderClosed, 'Native recorder stopped before its first PCM block');
      return size > 0;
    }, 'Native capture stream readiness', { timeoutMs });
    player = spawn('pw-cat', ['--playback', ...args, '--target', render, '-'], { stdio: ['pipe', 'ignore', 'pipe'] });
    player.once('close', () => { playerClosed = true; });
    player.on('error', value => { error = value; });
    player.stdin.on('error', value => { error = value; });
    player.stderr.resume();
    const pcm = Buffer.alloc(48000 * 4 * 8);
    for (let frame = 0; frame < 48000 * 4; frame++) for (let channel = 0; channel < 2; channel++) {
      pcm.writeFloatLE((channel + 1) * 0.125 * Math.sin(2 * Math.PI * (channel + 1) * 1000 * frame / 48000), frame * 8 + channel * 4);
    }
    player.stdin.end(pcm);
    // pw-cat exits after PipeWire drains the finite input, rather than when stdin
    // accepts it. Starting under TCG must not truncate the same four-second signal.
    await waitForSignalBoundary(() => {
      assert.ifError(error);
      assert.ok(!recorderClosed, 'Native recorder stopped during playback');
      return playerClosed;
    }, 'Native playback drain', { timeoutMs });
    assert.equal(player.signalCode, null, 'Native playback was interrupted');
    assert.equal(player.exitCode, 0, 'Native playback failed before drain');
  } finally {
    try {
      await stopSignalProcesses([player, recorder]);
      await waitForSignalBoundary(() => recorderClosed, 'Native capture pipe close', { timeoutMs: 5000 });
    } finally {
      writeFileSync(join(directory, 'native-signal.f32'), Buffer.concat(chunks), { flag: 'wx' });
    }
  }
  assert.ifError(error);
  const captured = Buffer.concat(chunks);
  const measured = analyseStereo(captured);
  assert.ok(Math.abs(measured.peaks[0] - 0.125) < 0.002 && Math.abs(measured.peaks[1] - 0.25) < 0.002,
    'Native loopback changed channel order or level');
  writeFileSync(join(directory, 'native-signal.json'), JSON.stringify(measured, null, 2) + '\n', { flag: 'wx' });
}

export async function nativeSignal(device, directory, os, { execute = execFileSync, diagnostics = collectMacosLoad,
  snapshot = snapshotMacosProcesses, schedulerTrace = false, trace = withMacosSchedulerTrace } = {}) {
  if (os === 'linux') return linuxSignal(device, directory);
  try { return await macosSignal(device, directory, execute, snapshot, schedulerTrace, trace); }
  catch (error) {
    try { diagnostics(directory); }
    catch (diagnosticError) { console.error(`Native PCM failure diagnostics: ${diagnosticError.message}`); }
    throw error;
  }
}

// Recorder finalization precedes diagnostic tools that suspend threads or consume CPU.
export async function runNativeAudioCases({ measure, finalize, directory,
  snapshot = snapshotMacosProcesses, diagnostics = collectMacosLoad }) {
  let snapshotPending = false, failurePending = false;
  try {
    await measure({ snapshot: () => { snapshotPending = true; },
      diagnostics: () => { failurePending = true; } });
  } finally {
    try { await finalize(); }
    finally {
      for (const [pending, collect] of [[snapshotPending, snapshot], [failurePending, diagnostics]]) {
        if (!pending) continue;
        try { collect(directory); }
        catch (error) { console.error(`Native post-recording diagnostics: ${error.message}`); }
      }
    }
  }
}

export function macosProbeCommand(bundle, directory, uid) {
  const args = ['-W', '-n', '-g', '--stdout', join(directory, 'native-probe.stdout.json'),
    '--stderr', join(directory, 'native-probe.stderr.log'), bundle, '--args', '--json', '--timing',
    '--capture-file', join(directory, 'native-signal.f32'),
    '--hal-trace-file', join(directory, 'native-hal-io.json'), '--seconds', '3', '--channels', '2', uid];
  return { program: 'open', args, timeout: 60000 };
}

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

async function macosSignal(device, directory, execFileSync, snapshot, schedulerTrace, trace) {
  const uid = device.nativeDeviceIds.find(id => id.startsWith('io.mixrelay.audio.endpoint.'));
  assert.ok(uid, 'Missing canonical CoreAudio device identity');
  const bundle = execFileSync('zsh', ['scripts/probe-macos-roundtrip.sh', '--prepare-bundle', 'io.mixrelay.qualification.probe'],
    { encoding: 'utf8', timeout: 60000 }).trim();
  const stdout = join(directory, 'native-probe.stdout.json');
  let processFailure;
  try {
    if (schedulerTrace) assert.equal(process.env.GITHUB_ACTIONS, 'true', 'Scheduler diagnosis requires a disposable hosted Mac');
    const command = macosProbeCommand(bundle, directory, uid);
    const measure = () => execFileSync(command.program, command.args, { timeout: command.timeout });
    if (schedulerTrace) await trace(directory, measure);
    else measure();
  } catch (error) {
    processFailure = error;
  } finally {
    const executable = `${bundle}/Contents/MacOS/MRRoundTripProbe`;
    const pattern = `^${executable.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}( |$)`;
    try { execFileSync('pkill', ['-TERM', '-f', pattern], { timeout: 5000 }); }
    catch (error) { if (error.status !== 1) throw error; }
  }
  let measured;
  try { measured = JSON.parse(readFileSync(stdout, 'utf8')); }
  catch (error) { throw new Error('Native macOS probe result could not be read', { cause: processFailure ?? error }); }
  if (measured.error) {
    const { stage, osStatus, errno } = measured.error;
    throw new Error(`Native macOS probe failed: stage=${stage} osStatus=${osStatus} errno=${errno}`, { cause: processFailure });
  }
  if (processFailure) throw processFailure;
  try { snapshot(directory); }
  catch (error) { console.error(`Native post-signal process snapshot: ${error.message}`); }
  assert.equal(measured.inputAuthorization?.final, 3, 'Probe recording consent must precede native I/O');
  assert.equal(measured.activityProtection, true, 'Finite measurement must retain its native activity');
  assert.equal(measured.complete, true);
  assert.equal(measured.channelOrder, true);
  for (const field of ['missingFrames', 'repeatedFrames', 'corruptFrames', 'nonfiniteSamples']) assert.equal(measured[field], 0, field);
  assert.ok(measured.receivedFrames >= 24000);
  writeFileSync(join(directory, 'native-signal.json'), JSON.stringify(measured, null, 2) + '\n', { flag: 'wx' });
}

export function analyseRecording(path) { return analyseRf64(readFileSync(path)); }
