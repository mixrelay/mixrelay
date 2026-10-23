import assert from 'node:assert/strict';
import test from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { analyseStereo, analyseRf64, stopSignalProcesses } from './native-signal.mjs';
import { analyseSystemMix } from './system-capture.mjs';

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
