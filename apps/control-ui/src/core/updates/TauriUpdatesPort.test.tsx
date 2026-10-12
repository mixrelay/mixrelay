import { invoke } from '@tauri-apps/api/core';
import { beforeEach, expect, it, vi } from 'vitest';

import { TauriUpdatesPort } from './TauriUpdatesPort';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

beforeEach(() => vi.mocked(invoke).mockReset());

it('checks and opens an installer through native catalog commands', async () => {
  const port = new TauriUpdatesPort();
  vi.mocked(invoke).mockResolvedValue(undefined);
  await port.check();
  await port.openDownload('mix-relay', 'https://downloads.example/driver.pkg');
  expect(vi.mocked(invoke).mock.calls).toEqual([
    ['updates_check'],
    [
      'updates_open_download',
      {
        product: 'mix-relay',
        url: 'https://downloads.example/driver.pkg',
      },
    ],
  ]);
});

it('does not create an update port or invoke native commands in a browser', async () => {
  const original = Object.getOwnPropertyDescriptor(window, '__TAURI_INTERNALS__');
  try {
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    vi.resetModules();
    const browser = await import('./TauriUpdatesPort');
    expect(browser.localUpdatesPort).toBeNull();
    expect(invoke).not.toHaveBeenCalled();

    Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {} });
    vi.resetModules();
    const native = await import('./TauriUpdatesPort');
    expect(native.localUpdatesPort).toBeInstanceOf(native.TauriUpdatesPort);
  } finally {
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    if (original) Object.defineProperty(window, '__TAURI_INTERNALS__', original);
  }
});
