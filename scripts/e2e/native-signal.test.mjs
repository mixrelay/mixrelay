import assert from 'node:assert/strict';
import test from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { EventEmitter } from 'node:events';
import { analyseStereo, analyseRf64, macosProbeCommand, nativeSignal, runNativeAudioCases, stopSignalProcesses, waitForSignalBoundary, withMacosSchedulerTrace } from './native-signal.mjs';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { analyseSystemMix } from './system-capture.mjs';

test('the LaunchServices measurement retains one original signal and unchanged audio settings', () => {
  const ordinary = macosProbeCommand('/probe.app', '/evidence', 'native-uid');
  assert.equal(ordinary.program, 'open');
  assert.equal(ordinary.args.filter(arg => arg === '--seconds').length, 1);
  assert.equal(ordinary.args[ordinary.args.indexOf('--seconds') + 1], '3');
  assert.equal(ordinary.args.some(arg => /buffer|warmup|retry/.test(arg)), false);
});

test('a scheduler trace cannot turn measured PCM loss into acceptance', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'mr-scheduler-diagnostic-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const before = process.env.GITHUB_ACTIONS;
  process.env.GITHUB_ACTIONS = 'true';
  const measured = { inputAuthorization: { final: 3 }, activityProtection: true, complete: true,
    channelOrder: true, missingFrames: 128, repeatedFrames: 0, corruptFrames: 128, nonfiniteSamples: 0, receivedFrames: 143872 };
  const calls = [];
  try {
    await assert.rejects(nativeSignal({ nativeDeviceIds: ['io.mixrelay.audio.endpoint.fixture'] }, directory, 'macos', {
      schedulerTrace: true, snapshot() {}, diagnostics() {},
      trace: async (_, measure) => { calls.push('trace-start'); measure(); calls.push('trace-stop'); },
      execute(program) {
        calls.push(program);
        if (program === 'zsh') return '/probe.app';
        if (program === 'open') writeFileSync(join(directory, 'native-probe.stdout.json'), JSON.stringify(measured));
      },
    }), /missingFrames/);
    assert.deepEqual(calls, ['zsh', 'trace-start', 'open', 'trace-stop', 'pkill']);
  } finally {
    if (before === undefined) delete process.env.GITHUB_ACTIONS;
    else process.env.GITHUB_ACTIONS = before;
  }
});

test('a native property failure retains its procedure and OSStatus and is never replayed', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'mr-property-failure-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  for (const processFailed of [false, true]) {
    const original = new Error('probe exited with status 1');
    const calls = [];
    await assert.rejects(nativeSignal({ nativeDeviceIds: ['io.mixrelay.audio.endpoint.fixture'] }, directory, 'macos', {
      snapshot() {}, diagnostics() {},
      execute(program) {
        calls.push(program);
        if (program === 'zsh') return '/probe.app';
        if (program === 'open') {
          writeFileSync(join(directory, 'native-probe.stdout.json'), JSON.stringify({
            error: { stage: 'hal_trace_enable', osStatus: 2003332927, errno: 0 },
          }));
          if (processFailed) throw original;
        }
      },
    }), error => {
      assert.match(error.message, /stage=hal_trace_enable osStatus=2003332927 errno=0/);
      assert.equal(error.cause, processFailed ? original : undefined);
      return true;
    });
    assert.deepEqual(calls, ['zsh', 'open', 'pkill']);
  }
});

test('system-wide tracing is ready before the original measurement and stops after success or failure', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'mr-system-trace-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const before = process.env.GITHUB_ACTIONS;
  process.env.GITHUB_ACTIONS = 'true';
  t.after(() => { if (before === undefined) delete process.env.GITHUB_ACTIONS; else process.env.GITHUB_ACTIONS = before; });
  for (const [failed, ready] of [[false, 'Recording started.'], [true, 'Starting recording with the System Trace template. Targeting All Processes.\nCtrl-C to stop the recording\n']]) {
    const phases = [];
    const child = new EventEmitter();
    child.stdout = new EventEmitter(); child.stderr = new EventEmitter();
    child.kill = signal => { phases.push(signal); child.emit('close', 0, null); };
    const work = withMacosSchedulerTrace(directory, () => {
      phases.push('measure');
      if (failed) throw new Error('original measurement failed');
      return 42;
    }, {
      start(program, args) {
        assert.equal(program, 'xcrun'); assert.ok(args.includes('--all-processes'));
        assert.equal(args.includes('--launch'), false);
        queueMicrotask(() => { phases.push('ready'); child.stdout.emit('data', Buffer.from(ready)); });
        return child;
      },
      wait: async read => { await Promise.resolve(); assert.equal(read(), true); },
    });
    if (failed) await assert.rejects(work, /original measurement failed/);
    else assert.equal(await work, 42);
    assert.deepEqual(phases, ['ready', 'measure', 'SIGINT']);
    // The diagnostic output name is reused only after this invocation has finished.
    rmSync(join(directory, 'scheduler.log'));
  }
});

function recording() {
  const header = Buffer.alloc(80);
  header.write('RF64'); header.writeUInt32LE(0xffffffff, 4); header.write('WAVE', 8);
  header.write('ds64', 12); header.writeUInt32LE(28, 16);
  header.writeBigUInt64LE(BigInt(80 + 48000 * 8 - 8), 20);
  header.writeBigUInt64LE(48000n * 8n, 28); header.writeBigUInt64LE(48000n, 36);
  header.write('fmt ', 48); header.writeUInt32LE(16, 52); header.writeUInt16LE(3, 56);
  header.writeUInt16LE(2, 58); header.writeUInt32LE(48000, 60); header.writeUInt32LE(384000, 64);
  header.writeUInt16LE(8, 68); header.writeUInt16LE(32, 70); header.write('data', 72); header.writeUInt32LE(0xffffffff, 76);
  const pcm = Buffer.alloc(48000 * 8);
  for (let frame = 0; frame < 48000; frame++) {
    pcm.writeFloatLE(Math.sin(frame * Math.PI / 24) * 0.125, frame * 8);
    pcm.writeFloatLE(Math.sin(frame * Math.PI / 12) * 0.25, frame * 8 + 4);
  }
  return Buffer.concat([header, pcm]);
}
test('finalized RF64 evidence contains finite native stereo PCM and consistent 64-bit sizes', () => {
  assert.equal(analyseRf64(recording()).frames, 48000);
  const unfinalized = recording(); unfinalized.writeBigUInt64LE(0n, 28);
  assert.throws(() => analyseRf64(unfinalized), /Unfinalized/);
  const wrongRate = recording(); wrongRate.writeUInt32LE(44100, 60);
  assert.throws(() => analyseRf64(wrongRate));
  assert.throws(() => analyseStereo(Buffer.alloc(48000 * 8)), /native stereo/);
  const nonfinite = recording().subarray(80); nonfinite.writeFloatLE(NaN, 0);
  assert.throws(() => analyseStereo(nonfinite), /Non-finite/);
  assert.throws(() => analyseRf64(recording().subarray(0, -4)));
});

test('system capture requires both independent frequencies in the correct stereo lanes', () => {
  assert.ok(analyseSystemMix(recording()).tones.left1000 > 0.12);
  const missing = recording();
  for (let offset = 84, frame = 0; offset < missing.length; offset += 8, frame++) missing.writeFloatLE(Math.sin(2 * Math.PI * 3000 * frame / 48000) * 0.1, offset);
  assert.throws(() => analyseSystemMix(missing), /Missing independent/);
  const swapped = recording();
  for (let offset = 80; offset < swapped.length; offset += 8) {
    const left = swapped.readFloatLE(offset), right = swapped.readFloatLE(offset + 4);
    swapped.writeFloatLE(right, offset); swapped.writeFloatLE(left, offset + 4);
  }
  assert.throws(() => analyseSystemMix(swapped), /Missing independent/);
});
test('the recorder uses WAVE_FORMAT_EXTENSIBLE for stereo Float32, including its exact float GUID', () => {
  const original = recording();
  const format = Buffer.alloc(48);
  original.copy(format, 0, 48, 72);
  format.writeUInt32LE(40, 4); format.writeUInt16LE(0xfffe, 8);
  format.writeUInt16LE(22, 24); format.writeUInt16LE(32, 26); format.writeUInt32LE(3, 28);
  Buffer.from('0300000000001000800000aa00389b71', 'hex').copy(format, 32);
  const extended = Buffer.concat([original.subarray(0, 48), format, original.subarray(72)]);
  extended.writeBigUInt64LE(BigInt(extended.length - 8), 20);
  assert.equal(analyseRf64(extended).frames, 48000);
  extended.writeUInt32LE(1, 80);
  assert.throws(() => analyseRf64(extended), /subformat/);
});

test('native probe cleanup reaps both owners when the first ignores termination', async t => {
  const children = [true, false].map(ignore => spawn(process.execPath, ['-e', `
    ${ignore ? "process.on('SIGTERM',()=>{});" : ''}
    setInterval(()=>{},1000); process.stdout.write('ready\\n');
  `], { stdio: ['ignore', 'pipe', 'ignore'] }));
  t.after(() => { for (const child of children) if (child.exitCode === null && child.signalCode === null) child.kill('SIGKILL'); });
  await Promise.all(children.map(child => once(child.stdout, 'data')));
  await assert.rejects(stopSignalProcesses(children, 100), /SIGKILL/);
  assert.equal(children[0].signalCode, 'SIGKILL');
  assert.ok(children.every(child => child.exitCode !== null || child.signalCode !== null), 'Both native probe owners must be reaped');
});

test('slow stream startup and drain are observed before analysing the same finite signal', async () => {
  let now = 0, frames = 0, played = false;
  await waitForSignalBoundary(() => played, 'playback drain', {
    timeoutMs: 1000, now: () => now,
    wait: async milliseconds => {
      now += milliseconds;
      if (now >= 400) { frames = 48000; played = true; }
    },
  });
  assert.equal(frames, 48000);
  assert.equal(now, 400, 'Do not return on a fixed startup or capture sleep');
});

test('a stalled stream fails within its bound and a process failure is never retried', async () => {
  let now = 0;
  await assert.rejects(waitForSignalBoundary(() => false, 'capture readiness', {
    timeoutMs: 100, now: () => now, wait: async milliseconds => { now += milliseconds; },
  }), /capture readiness.*deadline/);
  assert.equal(now, 100);
  let reads = 0;
  await assert.rejects(waitForSignalBoundary(() => { reads++; throw new Error('player exited 1'); }, 'playback drain'), /player exited 1/);
  assert.equal(reads, 1);
});

test('failed Mac PCM keeps the original verdict and collects host load only after the single signal ends', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'mr-native-failure-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const measured = { inputAuthorization: { final: 3 }, activityProtection: true, complete: true,
    channelOrder: true, missingFrames: 128, repeatedFrames: 0, corruptFrames: 0, nonfiniteSamples: 0, receivedFrames: 48000 };
  const phases = [];
  await assert.rejects(nativeSignal({ nativeDeviceIds: ['io.mixrelay.audio.endpoint.fixture'] }, directory, 'macos', {
    execute(program, args) {
      phases.push(program);
      if (program === 'zsh') return '/probe.app';
      if (program === 'open') {
        assert.equal(args[args.indexOf('--capture-file') + 1], join(directory, 'native-signal.f32'));
        writeFileSync(join(directory, 'native-probe.stdout.json'), JSON.stringify(measured));
      }
    },
    snapshot() { phases.push('snapshot'); },
    diagnostics(output) {
      assert.deepEqual(phases, ['zsh', 'open', 'pkill', 'snapshot']);
      assert.deepEqual(JSON.parse(readFileSync(join(output, 'native-probe.stdout.json'))), measured);
      phases.push('diagnostics');
      throw new Error('diagnostic collection unavailable');
    },
  }), /missingFrames/);
  assert.deepEqual(phases, ['zsh', 'open', 'pkill', 'snapshot', 'diagnostics']);
});

test('installed audio stops recording before post-signal observers, retaining the original PCM failure', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'mr-audio-order-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const measured = { inputAuthorization: { final: 3 }, activityProtection: true, complete: true,
    channelOrder: true, missingFrames: 636, repeatedFrames: 0, corruptFrames: 636, nonfiniteSamples: 0, receivedFrames: 143364 };
  let recording = true;
  const phases = [], verdicts = [];
  await runNativeAudioCases({ directory,
    async measure(observers) {
      try {
        await nativeSignal({ nativeDeviceIds: ['io.mixrelay.audio.endpoint.fixture'] }, directory, 'macos', {
          ...observers,
          execute(program) {
            if (program === 'zsh') return '/probe.app';
            if (program === 'open') {
              phases.push('finite signal');
              writeFileSync(join(directory, 'native-probe.stdout.json'), JSON.stringify(measured));
            }
          },
        });
      } catch (error) { verdicts.push(error.message); }
    },
    async finalize() { phases.push('recorder stopped'); recording = false; },
    snapshot() { phases.push('process snapshot'); assert.equal(recording, false); },
    diagnostics() { phases.push('thread samples'); assert.equal(recording, false); },
  });
  assert.deepEqual(phases, ['finite signal', 'recorder stopped', 'process snapshot', 'thread samples']);
  assert.equal(verdicts.length, 1);
  assert.match(verdicts[0], /missingFrames/);
});

test('unexpected measurement failure still finalizes recording before collecting requested diagnostics', async () => {
  const phases = [];
  await assert.rejects(runNativeAudioCases({ directory: '/unused',
    async measure(observers) { observers.snapshot(); throw new Error('measurement failed'); },
    async finalize() { phases.push('recorder stopped'); },
    snapshot() { phases.push('snapshot'); },
    diagnostics() { assert.fail('Successful or unrequested diagnostics must not run'); },
  }), /measurement failed/);
  assert.deepEqual(phases, ['recorder stopped', 'snapshot']);
});
