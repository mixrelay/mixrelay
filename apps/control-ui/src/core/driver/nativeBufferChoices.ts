import type { NativeBufferSettings } from '../control-port/types';

/** Range belongs to this exact runtime-owned endpoint, never the global DSP block. */
export function nativeBufferChoices(buffer: NativeBufferSettings): number[] {
  const { minimumFrames, maximumFrames } = buffer;
  if (
    !Number.isSafeInteger(minimumFrames) ||
    !Number.isSafeInteger(maximumFrames) ||
    minimumFrames < 1 ||
    maximumFrames < minimumFrames
  )
    return [];
  return [32, 64, 128, 256, 512, 1024, 2048, 4096]
    .filter(
      (frames) =>
        Number.isSafeInteger(frames) && frames >= minimumFrames && frames <= maximumFrames,
    )
    .sort((left, right) => left - right);
}
