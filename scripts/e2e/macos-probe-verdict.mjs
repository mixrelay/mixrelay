import assert from 'node:assert/strict';

const PCM_COUNTERS = ['missingFrames', 'repeatedFrames', 'corruptFrames', 'nonfiniteSamples'];

// The boundary names the measured OS round trip; it does not assign its fault to HAL or the client.
class MacosPcmFailure extends Error {
  constructor(measured) {
    const counters = Object.fromEntries(PCM_COUNTERS.map(field => [field, measured[field]]));
    super(`macOS/CoreAudio/os_roundtrip: ${Object.entries(counters).map(([field, count]) => `${field}=${count}`).join(' ')}`);
    this.name = 'MacosPcmFailure';
    this.measurementFailure = Object.freeze({ os: 'macos', backend: 'CoreAudio', boundary: 'os_roundtrip', ...counters });
  }
}

export function verifyMeasurement(measured) {
  assert.equal(measured.inputAuthorization?.final, 3, 'Probe recording consent must precede native I/O');
  assert.equal(measured.inputAuthorization?.initial, 3, 'The unchanged signed probe must retain its preflight consent');
  assert.equal(measured.inputAuthorization?.requested, false, 'Finite PCM cannot request consent while observation is paused');
  assert.equal(measured.activityProtection, true, 'Finite measurement must retain its native activity');
  assert.equal(measured.complete, true);
  assert.equal(measured.channelOrder, true);
  for (const field of PCM_COUNTERS) assert.ok(Number.isSafeInteger(measured[field]) && measured[field] >= 0, field);
  if (PCM_COUNTERS.some(field => measured[field] !== 0)) throw new MacosPcmFailure(measured);
  assert.ok(measured.receivedFrames >= 24000);
}
