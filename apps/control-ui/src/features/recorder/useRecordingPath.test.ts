import { act, renderHook, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import type { ControlPort, RecorderStatus } from '../../core/control-port/types';
import { useRecordingPath } from './useRecordingPath';

const stopped: RecorderStatus = {
  state: 'stopped',
  path: null,
  frames: 0,
  sampleRate: null,
  encoding: null,
  error: null,
};
const failed: RecorderStatus = {
  ...stopped,
  state: 'failed',
  path: '/Desktop/first.wav',
  frames: 96000,
  error: 'disk write failed',
};

it('prepares another automatic path after a failed take without erasing its status', async () => {
  const resolve = vi
    .fn()
    .mockResolvedValueOnce('/Desktop/first.wav')
    .mockResolvedValue('/Desktop/second.wav');
  const port = { defaultRecordingPath: resolve } as unknown as ControlPort;
  const { result, rerender } = renderHook(({ status }) => useRecordingPath(port, status), {
    initialProps: { status: stopped },
  });
  await waitFor(() => expect(result.current.path).toBe('/Desktop/first.wav'));
  rerender({ status: failed });
  await waitFor(() => expect(result.current.path).toBe('/Desktop/second.wav'));
  expect(failed.error).toBe('disk write failed');
  expect(resolve).toHaveBeenCalledTimes(2);
});

it('does not replace manual destinations when a take fails', async () => {
  const resolve = vi.fn().mockResolvedValue('/Desktop/first.wav');
  const port = { defaultRecordingPath: resolve } as unknown as ControlPort;
  const { result, rerender } = renderHook(({ status }) => useRecordingPath(port, status), {
    initialProps: { status: stopped },
  });
  await waitFor(() => expect(result.current.path).toBe('/Desktop/first.wav'));
  act(() => result.current.setPath('/manual/chosen.wav'));
  rerender({ status: failed });
  expect(result.current.path).toBe('/manual/chosen.wav');
  expect(resolve).toHaveBeenCalledOnce();
});

it('ignores a previous port owner default path after switching owners', async () => {
  let finish: (path: string) => void = () => undefined;
  const previous = {
    defaultRecordingPath: () =>
      new Promise<string>((resolve) => {
        finish = resolve;
      }),
  } as unknown as ControlPort;
  const next = {
    defaultRecordingPath: vi.fn().mockResolvedValue('/new/Desktop/take.wav'),
  } as unknown as ControlPort;
  const { result, rerender } = renderHook(({ port }) => useRecordingPath(port, stopped), {
    initialProps: { port: previous },
  });
  rerender({ port: next });
  await waitFor(() => expect(result.current.path).toBe('/new/Desktop/take.wav'));
  await act(async () => {
    finish('/old/Desktop/take.wav');
  });
  expect(result.current.path).toBe('/new/Desktop/take.wav');
});
