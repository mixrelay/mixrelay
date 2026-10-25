import assert from 'node:assert/strict';
import test from 'node:test';
import { analyseHalIo, probeSignalWindow } from './analyse-hal-io.mjs';

const a = '1234567890abcdef', b = 'fedcba0987654321';
const trace = rows => ({ version: 1, capacity: 2048, issued: rows.length, dropped: 0, rows });
const window = { start: 100, end: 108 };

test('published digital silence remains distinct from an unavailable capture', () => {
  const published = analyseHalIo(trace([[1, 1, 100, 8, 1, 8, 8, a], [2, 0, 100, 8, 1, 8, 8, a]]), window);
  assert.equal(published.writeSilence.length, 1);
  assert.equal(published.suppliedCaptureSilence.length, 1);
  assert.equal(published.captureShortReads.length, 0);
  assert.equal(published.exactCoordinatePairs, 1);
  const unavailable = analyseHalIo(trace([[1, 1, 100, 8, 1, 8, 0, a], [2, 0, 100, 8, 1, 4, 4, b]]), window);
  assert.equal(unavailable.writeSilence.length, 0);
  assert.equal(unavailable.captureShortReads[0].available, 4);
  assert.equal(unavailable.suppliedCaptureSilence.length, 0);
  assert.equal(unavailable.suppliedHashDifferences.length, 0);
});

test('fully supplied coordinate pairs retain a hash difference without assigning an epoch or cause', () => {
  const result = analyseHalIo(trace([[1, 1, 100, 8, 1, 8, 0, a], [2, 0, 100, 8, 1, 8, 0, b]]), window);
  assert.equal(result.suppliedHashDifferences.length, 1);
  assert.equal(result.suppliedHashDifferences[0].write.hash, a);
  assert.ok(result.limitations.some(line => line.includes('epochs')));
});

test('an incomplete snapshot and partial signal overlap remain explicit', () => {
  const input = trace([[3, 0, 96, 8, 1, 4, 4, a]]);
  input.issued = 2050; input.dropped = 2;
  const result = analyseHalIo(input, window);
  assert.equal(result.retention.notRetainedRecentRows, 2047);
  assert.equal(result.retention.observationsBeyondCapacity, 2);
  assert.equal(result.retention.writerDropped, 2);
  assert.equal(result.captureShortReads[0].signalOverlap, 'partial');
  assert.equal(result.unpairedCaptureRows, 1);
});

test('invalid schemas and duplicate sequences cannot become trace evidence', () => {
  assert.throws(() => analyseHalIo({ ...trace([]), version: 3 }));
  assert.throws(() => analyseHalIo(trace([[1, 0, 100, 8, 1, 9, 0, a]])));
  assert.throws(() => analyseHalIo(trace([[1, 0, 100, 8, 1, 8, 0, a], [1, 1, 100, 8, 1, 8, 0, a]])));
});

test('client output hashes remain distinct from a multi-client capture mix in v2', () => {
  const input = { version: 2, capacity: 4096, issued: 3, dropped: 0, rows: [
    [1, 1, 100, 8, 1, 8, 0, a, 42], [2, 1, 100, 8, 1, 8, 0, b, 43],
    [3, 0, 100, 8, 1, 8, 0, 'aabbccddeeff0011', 0],
  ] };
  const result = analyseHalIo(input, window);
  assert.deepEqual(result.writeClients, [42, 43]);
  assert.equal(result.captureHashComparison, 'inapplicable_multiple_client_mix');
  assert.deepEqual(result.suppliedHashDifferences, []);
  assert.equal(result.retention.retainedRows, 3);
  assert.throws(() => analyseHalIo({ ...input, capacity: 2048 }));
});

test('an empty or rejected oversized callback remains observable instead of being discarded', () => {
  const result = analyseHalIo(trace([[1, 0, 100, 0, 1, 0, 0, a], [2, 1, 100, 8192, 0, 0, 0, b]]));
  assert.equal(result.emptyTransfers, 1);
  assert.equal(result.writeRejections[0].frames, 8192);
});

test('the active signal window follows original output coordinates and excludes lead and tail', () => {
  const timing = 'input recording consent complete\nside,first,sampleTime,hostTime,frames,flags,callbackEnter,callbackExit\noutput,512,20512,9000,512,3,1,2\n';
  assert.deepEqual(probeSignalWindow({ output: { rate: 48000 }, signalFrames: 144000 }, timing), { start: 24800, end: 168800 });
  assert.equal(probeSignalWindow({ output: { rate: 48000 }, signalFrames: 144000 }, ''), null);
});
