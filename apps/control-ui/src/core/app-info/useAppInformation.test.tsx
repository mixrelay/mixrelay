import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import { version } from '../../../package.json';
import { useAppInformation } from './useAppInformation';

const native = vi.hoisted(() => ({ isTauri: vi.fn(), getVersion: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/app', () => ({ getVersion: native.getVersion }));
vi.mock('@tauri-apps/api/core', () => ({ isTauri: native.isTauri }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));

beforeEach(() => {
  vi.resetAllMocks();
  native.isTauri.mockReturnValue(true);
  native.getVersion.mockResolvedValue('1.2.3');
  native.listen.mockResolvedValue(vi.fn());
});

it('reads the installed version and opens the latest About callback from the native menu', async () => {
  const stop = vi.fn();
  native.listen.mockResolvedValue(stop);
  const first = vi.fn();
  const latest = vi.fn();
  const view = renderHook(({ open }) => useAppInformation(open, true), {
    initialProps: { open: first },
  });
  await waitFor(() => expect(view.result.current).toBe('1.2.3'));
  expect(native.listen).toHaveBeenCalledWith('app-about', expect.any(Function));
  view.rerender({ open: latest });
  act(() => native.listen.mock.calls[0]?.[1]());
  expect(latest).toHaveBeenCalledOnce();
  expect(first).not.toHaveBeenCalled();
  view.unmount();
  expect(stop).toHaveBeenCalledOnce();
});

it('releases a native listener that finishes registering after unmount', async () => {
  let register: (stop: () => void) => void = () => {};
  native.listen.mockReturnValue(
    new Promise<() => void>((resolve) => {
      register = resolve;
    }),
  );
  const view = renderHook(() => useAppInformation(vi.fn(), true));
  view.unmount();
  const stop = vi.fn();
  await act(async () => register(stop));
  expect(stop).toHaveBeenCalledOnce();
});

it('uses the build version in the browser without calling native APIs', () => {
  native.isTauri.mockReturnValue(false);
  const view = renderHook(() => useAppInformation(vi.fn(), true));
  expect(view.result.current).toBe(version);
  expect(native.getVersion).not.toHaveBeenCalled();
  expect(native.listen).not.toHaveBeenCalled();
});

it('keeps product information usable when native version and listener requests fail', async () => {
  const versionError = new Error('Native version unavailable');
  const listenerError = new Error('Native menu unavailable');
  native.getVersion.mockRejectedValue(versionError);
  native.listen.mockRejectedValue(listenerError);
  const errors = vi.spyOn(console, 'error').mockImplementation(() => {});
  try {
    const view = renderHook(() => useAppInformation(vi.fn(), true));
    await waitFor(() => expect(errors).toHaveBeenCalledTimes(2));
    expect(view.result.current).toBe(version);
    expect(errors).toHaveBeenCalledWith('Could not read app version', versionError);
    expect(errors).toHaveBeenCalledWith('About menu listener failed', listenerError);
  } finally {
    errors.mockRestore();
  }
});
