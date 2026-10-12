import { invoke } from '@tauri-apps/api/core';
import { beforeEach, expect, it, vi } from 'vitest';
import type { DriverCommand } from './DriverPort';
import { TauriDriverPort } from './TauriDriverPort';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
beforeEach(() => {
  vi.mocked(invoke).mockReset();
});

it('uses only Driver owner commands and full directional formats', async () => {
  const port = new TauriDriverPort();
  const command = {
    action: 'set_format' as const,
    endpointId: 'device-a',
    expectedRevision: 7,
    direction: 'capture' as const,
    format: { sampleRate: 48000, channels: 2, sampleFormat: 'signed32' as const },
  };
  await port.getSnapshot();
  await port.mutate(command);
  expect(vi.mocked(invoke).mock.calls).toEqual([['driver_status'], ['driver_mutate', { command }]]);
});

it('sends the device sample format without directional format settings', async () => {
  const command: DriverCommand = {
    action: 'set_sample_format',
    endpointId: 'device-a',
    expectedRevision: 7,
    sampleFormat: 'signed24',
  };
  await new TauriDriverPort().mutate(command);
  expect(invoke).toHaveBeenCalledExactlyOnceWith('driver_mutate', {
    command: {
      action: 'set_sample_format',
      endpointId: 'device-a',
      expectedRevision: 7,
      sampleFormat: 'signed24',
    },
  });
});

it('has no local Driver service port in a browser preview', async () => {
  const original = Object.getOwnPropertyDescriptor(window, '__TAURI_INTERNALS__');
  try {
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    vi.resetModules();
    expect((await import('./TauriDriverPort')).localDriverPort).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  } finally {
    if (original) Object.defineProperty(window, '__TAURI_INTERNALS__', original);
  }
});

it('exposes the standalone Driver owner in a packaged app and propagates owner failures', async () => {
  const original = Object.getOwnPropertyDescriptor(window, '__TAURI_INTERNALS__');
  try {
    Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {} });
    vi.resetModules();
    const { localDriverPort } = await import('./TauriDriverPort');
    expect(localDriverPort).toBeInstanceOf((await import('./TauriDriverPort')).TauriDriverPort);
    if (!localDriverPort) throw new Error('Driver owner was not available');
    const failure = new Error('Driver is unavailable');
    vi.mocked(invoke).mockRejectedValueOnce(failure);
    await expect(localDriverPort.getSnapshot()).rejects.toBe(failure);
  } finally {
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    if (original) Object.defineProperty(window, '__TAURI_INTERNALS__', original);
    vi.resetModules();
  }
});
