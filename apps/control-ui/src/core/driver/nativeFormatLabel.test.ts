import { describe, expect, it } from 'vitest';

import { nativeFormatKey, nativeFormatLabel } from './nativeFormatLabel';

describe('native device format presentation', () => {
  it.each([
    ['signed16', '16-bit PCM'],
    ['signed24', '24-bit PCM'],
    ['signed32', '32-bit PCM'],
    ['float32', '32-bit Float'],
  ] as const)('distinguishes %s in the key and readable label', (sampleFormat, label) => {
    const format = { sampleRate: 44100, channels: 8, sampleFormat };
    expect(nativeFormatKey(format)).toBe(`44100:8:${sampleFormat}`);
    expect(nativeFormatLabel(format)).toBe(`44.1 kHz · 8 ch · ${label}`);
  });
});
