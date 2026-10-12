import { expect, it } from 'vitest';

import type { NativeBufferSettings } from '../control-port/types';
import { nativeBufferChoices } from './nativeBufferChoices';

const buffer: NativeBufferSettings = {
  currentFrames: 192,
  preferredFrames: 384,
  minimumFrames: 96,
  maximumFrames: 768,
  canSet: true,
};

it('offers supported presets within one device range without inventing nonpreset choices', () => {
  expect(nativeBufferChoices(buffer)).toEqual([128, 256, 512]);
  expect(nativeBufferChoices({ ...buffer, currentFrames: 128, preferredFrames: 128 })).toEqual([
    128, 256, 512,
  ]);
  expect(nativeBufferChoices({ ...buffer, currentFrames: null, preferredFrames: null })).toEqual([
    128, 256, 512,
  ]);
  expect(nativeBufferChoices({ ...buffer, currentFrames: 0, preferredFrames: 2048 })).toEqual([
    128, 256, 512,
  ]);
});

it.each([
  { minimumFrames: 0 },
  { minimumFrames: Number.NaN },
  { maximumFrames: Number.POSITIVE_INFINITY },
  { maximumFrames: 32 },
])('refuses malformed capabilities: %o', (change) => {
  expect(nativeBufferChoices({ ...buffer, ...change })).toEqual([]);
});
