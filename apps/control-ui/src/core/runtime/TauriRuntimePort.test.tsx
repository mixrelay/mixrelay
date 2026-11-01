import { Channel, invoke } from '@tauri-apps/api/core';
import { afterEach, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  Channel: class {
    constructor(readonly onmessage: (message: unknown) => void) {}
  },
}));

const original = Object.getOwnPropertyDescriptor(window, '__TAURI_INTERNALS__');
afterEach(() => {
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
  if (original) Object.defineProperty(window, '__TAURI_INTERNALS__', original);
  vi.mocked(invoke).mockReset();
  vi.resetModules();
});

it('does not contact a native runtime in a browser preview', async () => {
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
  vi.resetModules();
  expect((await import('./TauriRuntimePort')).localRuntimePort).toBeNull();
  expect(invoke).not.toHaveBeenCalled();
});

it('observes runtime status without restarting it and connects only on an explicit request', async () => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {} });
  vi.resetModules();
  const { localRuntimePort } = await import('./TauriRuntimePort');
  if (!localRuntimePort) throw new Error('Native runtime adapter was not available');
  vi.mocked(invoke).mockResolvedValueOnce('unavailable').mockResolvedValueOnce('ready');
  await expect(localRuntimePort.status()).resolves.toBe('unavailable');
  expect(invoke).toHaveBeenCalledExactlyOnceWith('runtime_status');
  const onStage = vi.fn();
  await expect(localRuntimePort.connect(onStage)).resolves.toBe('ready');
  expect(invoke).toHaveBeenLastCalledWith('runtime_connect', { onStage: expect.any(Channel) });
});

it('forwards known connection steps and ignores anything else on the channel', async () => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {} });
  vi.resetModules();
  const { localRuntimePort } = await import('./TauriRuntimePort');
  if (!localRuntimePort) throw new Error('Native runtime adapter was not available');
  vi.mocked(invoke).mockResolvedValueOnce('ready');
  const onStage = vi.fn();
  await localRuntimePort.connect(onStage);
  const [, args] = vi.mocked(invoke).mock.lastCall ?? [];
  const channel = (args as { onStage: { onmessage: (message: unknown) => void } }).onStage;
  for (const message of ['starting', 'restarting', 3, null, 'preparing_engine'])
    channel.onmessage(message);
  expect(onStage.mock.calls).toEqual([['starting'], ['preparing_engine']]);
});

it('refuses an outcome that is not a runtime status', async () => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {} });
  vi.resetModules();
  const { localRuntimePort } = await import('./TauriRuntimePort');
  if (!localRuntimePort) throw new Error('Native runtime adapter was not available');
  vi.mocked(invoke).mockResolvedValueOnce('starting').mockResolvedValueOnce({ status: 'ready' });
  await expect(localRuntimePort.status()).rejects.toThrow('invalid response at runtime_status');
  await expect(localRuntimePort.connect(vi.fn())).rejects.toThrow('runtime_connect');
});

it('preserves native connection failures for the connection screen', async () => {
  Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {} });
  vi.resetModules();
  const { localRuntimePort } = await import('./TauriRuntimePort');
  if (!localRuntimePort) throw new Error('Native runtime adapter was not available');
  const failure = new Error('Driver is not installed');
  vi.mocked(invoke).mockRejectedValueOnce(failure);
  await expect(localRuntimePort.connect(vi.fn())).rejects.toBe(failure);
});
