import assert from 'node:assert/strict';
import test from 'node:test';
import { analyseProbeReference, probeRangeHash, probeSampleBits } from './probe-pcm-reference.mjs';

test('integer reference matches a full healthy HAL write hashed by the native probe candidate', () => {
  // Run 40220258479, macOS 14 ARM fresh: device 100996, output origin 54916.
  assert.equal(probeRangeHash(46080, 512, 2), 'af01f59b78217b25');
});

test('original input mismatch retains exact coordinates and distinguishes zero from wrong PCM', () => {
  const pcm = Buffer.alloc(8 * 2 * 4);
  for (let frame = 0; frame < 8; frame++) for (let channel = 0; channel < 2; channel++) {
    pcm.writeUInt32LE(probeSampleBits(frame + 4, channel), (frame * 2 + channel) * 4);
  }
  const metrics = { sameDevice: true, output: { rate: 40, channels: 2 }, input: { rate: 40, channels: 2 }, signalFrames: 8, recordedFrames: 8 };
  const timing = 'output,0,100,10,8,1,1,2\ninput,0,104,10,8,1,1,2\n';
  const write = [1, 1, 104, 8, 1, 8, 0, probeRangeHash(4, 8, 2)];
  const trace = { rows: [write] };
  assert.deepEqual(analyseProbeReference(trace, metrics, timing, pcm).inputDifferences, []);
  pcm.fill(0, 16, 24);
  pcm.writeFloatLE(0.5, 32);
  const result = analyseProbeReference(trace, metrics, timing, pcm);
  assert.deepEqual(result.nativeWrites.differences, []);
  assert.deepEqual(result.inputDifferences.map(({ deviceStart, deviceEnd, zeroFrames, retainedWriteRows }) =>
    [deviceStart, deviceEnd, zeroFrames, retainedWriteRows.length]), [[106, 107, 1, 1], [108, 109, 0, 1]]);
  assert.throws(() => analyseProbeReference(trace, metrics, timing, pcm.subarray(4)));
});
