import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import { useNativeGraphListCommand } from './useNativeGraphListCommand';

const native = vi.hoisted(() => ({ isTauri: vi.fn(), invoke: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ isTauri: native.isTauri, invoke: native.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));

beforeEach(() => {
  vi.resetAllMocks();
  native.isTauri.mockReturnValue(true);
  native.invoke.mockResolvedValue(undefined);
  native.listen.mockResolvedValue(vi.fn());
});

it('runs the latest toggle from the View menu and mirrors visibility to its label', async () => {
  const stop = vi.fn();
  native.listen.mockResolvedValue(stop);
  const first = vi.fn();
  const latest = vi.fn();
  const view = renderHook(({ visible, toggle }) => useNativeGraphListCommand(visible, toggle), {
    initialProps: { visible: true, toggle: first },
  });
  await waitFor(() =>
    expect(native.listen).toHaveBeenCalledWith('graph-list-toggle', expect.any(Function)),
  );
  expect(native.invoke).toHaveBeenLastCalledWith('set_graph_list_visible', { visible: true });
  view.rerender({ visible: false, toggle: latest });
  expect(native.invoke).toHaveBeenLastCalledWith('set_graph_list_visible', { visible: false });
  act(() => native.listen.mock.calls[0]?.[1]());
  expect(latest).toHaveBeenCalledOnce();
  expect(first).not.toHaveBeenCalled();
  expect(native.listen).toHaveBeenCalledOnce();
  view.unmount();
  await waitFor(() => expect(stop).toHaveBeenCalledOnce());
});

it('releases a listener that registers after unmount and stays inert outside the app', async () => {
  let register: (stop: () => void) => void = () => {};
  native.listen.mockReturnValue(
    new Promise<() => void>((resolve) => {
      register = resolve;
    }),
  );
  const view = renderHook(() => useNativeGraphListCommand(true, vi.fn()));
  view.unmount();
  const stop = vi.fn();
  await act(async () => register(stop));
  expect(stop).toHaveBeenCalledOnce();

  vi.resetAllMocks();
  native.isTauri.mockReturnValue(false);
  renderHook(() => useNativeGraphListCommand(true, vi.fn()));
  expect(native.listen).not.toHaveBeenCalled();
  expect(native.invoke).not.toHaveBeenCalled();
});

it('logs native listener and label failures without throwing', async () => {
  const error = vi.spyOn(console, 'error').mockImplementation(() => {});
  try {
    native.listen.mockRejectedValue(new Error('no event bus'));
    native.invoke.mockRejectedValue(new Error('no command'));
    renderHook(() => useNativeGraphListCommand(true, vi.fn()));
    await waitFor(() => expect(error).toHaveBeenCalledTimes(2));
    expect(error).toHaveBeenCalledWith('Graph list menu listener failed', expect.any(Error));
    expect(error).toHaveBeenCalledWith('Graph list menu label failed', expect.any(Error));
  } finally {
    error.mockRestore();
  }
});
