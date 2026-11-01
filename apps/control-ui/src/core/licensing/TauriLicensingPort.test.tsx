import { invoke } from '@tauri-apps/api/core';
import { beforeEach, expect, it, vi } from 'vitest';

import { TauriLicensingPort } from './TauriLicensingPort';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset();
});

const free = { status: 'free', expiresAtUnix: null, remainingDays: 0, configured: true };

it('uses only the native licensing commands and passes a key only to activation', async () => {
  const port = new TauriLicensingPort();
  vi.mocked(invoke).mockResolvedValue(free);
  await port.getStatus();
  await port.activate('secret-license-key', 'Studio Mac');
  await port.validate();
  await port.deactivate();
  await port.openCheckout();
  expect(vi.mocked(invoke).mock.calls).toEqual([
    ['licensing_status'],
    ['licensing_activate', { key: 'secret-license-key', instanceName: 'Studio Mac' }],
    ['licensing_validate'],
    ['licensing_deactivate'],
    ['licensing_open_checkout'],
  ]);
});

it('rejects a status that is not a licensing state, including an unknown access state', async () => {
  const port = new TauriLicensingPort();
  for (const reply of [undefined, { ...free, status: 'trial' }, { ...free, remainingDays: -1 }]) {
    vi.mocked(invoke).mockResolvedValueOnce(reply);
    await expect(port.getStatus()).rejects.toThrow('invalid response at licensing_status');
  }
  vi.mocked(invoke).mockResolvedValueOnce({
    ...free,
    status: 'licensed',
    expiresAtUnix: 9,
    hasActivation: true,
  });
  await expect(port.validate()).resolves.toMatchObject({ status: 'licensed', hasActivation: true });
});

it('selects a native port only when Tauri internals exist', async () => {
  const original = Object.getOwnPropertyDescriptor(window, '__TAURI_INTERNALS__');
  try {
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    vi.resetModules();
    const browserModule = await import('./TauriLicensingPort');
    expect(browserModule.localLicensingPort).toBeNull();

    Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: {} });
    vi.resetModules();
    const nativeModule = await import('./TauriLicensingPort');
    expect(nativeModule.localLicensingPort).toBeInstanceOf(nativeModule.TauriLicensingPort);
  } finally {
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    if (original) Object.defineProperty(window, '__TAURI_INTERNALS__', original);
  }
});
