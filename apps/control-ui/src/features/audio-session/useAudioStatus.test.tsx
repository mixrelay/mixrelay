import { act, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { ControlPort, MixRelayStatus } from '../../core/control-port/types';
import { useAudioStatus } from './useAudioStatus';

afterEach(() => vi.useRealTimers());
const observed: MixRelayStatus = {
  protocolVersion: 1,
  driverState: 'ready',
  sampleRate: 44100,
  bufferFrames: 128,
  underruns: 0,
  overruns: 0,
};

it('ignores a pre-change native response after explicit device refresh and cancels polling on unmount', async () => {
  vi.useFakeTimers();
  let resolveOld!: (value: MixRelayStatus) => void;
  const getStatus = vi
    .fn()
    .mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
    )
    .mockResolvedValue(observed);
  const port = { getStatus } as unknown as ControlPort;
  const { result, rerender, unmount } = renderHook(({ refresh }) => useAudioStatus(port, refresh), {
    initialProps: { refresh: 0 },
  });
  await act(async () => {
    rerender({ refresh: 1 });
  });
  expect(result.current).toEqual(observed);
  await act(async () => {
    resolveOld({ ...observed, sampleRate: 48000 });
  });
  expect(result.current).toEqual(observed);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
  });
  expect(getStatus).toHaveBeenCalledTimes(3);
  unmount();
  await vi.advanceTimersByTimeAsync(5000);
  expect(getStatus).toHaveBeenCalledTimes(3);
  expect(vi.getTimerCount()).toBe(0);
});

it('clears stale healthy observations when the native status request fails', async () => {
  const getStatus = vi
    .fn()
    .mockResolvedValueOnce(observed)
    .mockRejectedValueOnce(new Error('offline'));
  const port = { getStatus } as unknown as ControlPort;
  const { result, rerender } = renderHook(({ refresh }) => useAudioStatus(port, refresh), {
    initialProps: { refresh: 0 },
  });
  await act(async () => {});
  expect(result.current).toEqual(observed);
  await act(async () => {
    rerender({ refresh: 1 });
  });
  expect(result.current).toBeNull();
});
