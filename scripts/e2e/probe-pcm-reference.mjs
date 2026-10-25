import assert from 'node:assert/strict';

// MRRoundTripProbe's integer-coded signal, before lead/tail placement.
export function probeSampleBits(frame, channel) {
  let value = ((Math.floor(frame / 4) >>> 0) ^ Math.imul(channel + 1, 0x9e3779b9)) >>> 0;
  value ^= value >>> 16;
  value = Math.imul(value, 0x7feb352d);
  value ^= value >>> 15;
  value = Math.imul(value, 0x846ca68b);
  value ^= value >>> 16;
  const payload = (value >>> ((frame % 4) * 8)) & 255;
  let code = ((payload << 6) | channel) - 8192;
  if (code >= 0) code++;
  const magnitude = Math.abs(code), exponent = 31 - Math.clz32(magnitude);
  return ((code < 0 ? 0x80000000 : 0) | ((exponent + 112) << 23)
    | ((magnitude - 2 ** exponent) << (23 - exponent))) >>> 0;
}

export function probeRangeHash(start, frames, channels) {
  let hash = 0xcbf29ce484222325n;
  for (let frame = start; frame < start + frames; frame++) {
    for (let channel = 0; channel < channels; channel++) {
      hash = BigInt.asUintN(64, (hash ^ BigInt(probeSampleBits(frame, channel))) * 0x100000001b3n);
    }
  }
  return hash.toString(16).padStart(16, '0');
}

export function analyseProbeReference(trace, metrics, timing, pcm) {
  assert.equal(metrics.sameDevice, true, 'Reference requires the same native device');
  assert.equal(metrics.output.rate, metrics.input.rate, 'Reference requires equal native rates');
  assert.equal(metrics.output.channels, metrics.input.channels);
  const channels = metrics.input.channels;
  assert.ok(Number.isSafeInteger(channels) && channels > 0 && channels <= 64);
  assert.equal(pcm.length, metrics.recordedFrames * channels * 4, 'Incomplete original Float32 capture');
  const rows = timing.split('\n').filter(line => /^(input|output),/.test(line)).map(line => {
    const [side, ...numbers] = line.split(',');
    assert.equal(numbers.length, 7, 'Unsupported original probe timing schema');
    const [first, sample, host, frames, flags, enter, exit] = numbers.map(Number);
    assert.ok([first, sample, host, frames, flags, enter, exit].every(value => Number.isSafeInteger(value) && value >= 0));
    return { side, first, sample, frames, flags };
  });
  const first = rows.find(row => row.side === 'output');
  assert.ok(first && (first.flags & 1), 'Missing valid output origin');
  const origin = first.sample - first.first;
  const start = origin + Math.trunc(metrics.output.rate * 0.1), end = start + metrics.signalFrames;
  const writes = trace.rows.filter(row => row[1] === 1 && row[2] >= start && row[2] + row[3] <= end);
  const differences = [];
  for (const row of writes) {
    const expected = probeRangeHash(row[2] - origin, row[3], channels);
    if (row[7] !== expected) differences.push({ sequence: row[0], frame: row[2], frames: row[3], observed: row[7], expected });
  }
  const inputDifferences = [];
  for (const row of rows.filter(row => row.side === 'input')) {
    assert.ok((row.flags & 1) && row.first + row.frames <= metrics.recordedFrames, 'Invalid input capture coordinates');
    let mismatch = null;
    const flush = () => {
      if (mismatch) inputDifferences.push(mismatch);
      mismatch = null;
    };
    for (let frame = 0; frame < row.frames; frame++) {
      const device = row.sample + frame, recorded = row.first + frame;
      if (device < start || device >= end) { flush(); continue; }
      let wrong = false, zero = true;
      for (let channel = 0; channel < channels; channel++) {
        const bits = pcm.readUInt32LE((recorded * channels + channel) * 4);
        wrong ||= bits !== probeSampleBits(device - origin, channel);
        zero &&= (bits & 0x7fffffff) === 0;
      }
      if (!wrong) { flush(); continue; }
      mismatch ??= { deviceStart: device, deviceEnd: device, recordedStart: recorded, frames: 0, zeroFrames: 0 };
      mismatch.deviceEnd++;
      mismatch.frames++;
      mismatch.zeroFrames += Number(zero);
    }
    flush();
  }
  for (const mismatch of inputDifferences) {
    mismatch.retainedCaptureRows = trace.rows.filter(row => row[1] === 0
      && row[2] < mismatch.deviceEnd && row[2] + row[3] > mismatch.deviceStart);
    mismatch.retainedWriteRows = trace.rows.filter(row => row[1] === 1
      && row[2] < mismatch.deviceEnd && row[2] + row[3] > mismatch.deviceStart);
  }
  return { origin, signalWindow: { start, end }, nativeWrites: { comparedRows: writes.length, differences }, inputDifferences,
    limitations: ['Reference applies to the integer-coded default probe signal and its original timing/capture files.',
      'Native trace retention and missing epoch labels still limit attribution. This comparison does not change qualification.'] };
}
