import type { NativeStreamFormat } from '../control-port/types';

export function nativeFormatKey(format: NativeStreamFormat): string {
  return `${format.sampleRate}:${format.channels}:${format.sampleFormat}`;
}

export function nativeFormatLabel(format: NativeStreamFormat): string {
  const representation = {
    float32: '32-bit Float', signed16: '16-bit PCM', signed24: '24-bit PCM', signed32: '32-bit PCM',
  }[format.sampleFormat];
  return `${format.sampleRate / 1_000} kHz · ${format.channels} ch · ${representation}`;
}
