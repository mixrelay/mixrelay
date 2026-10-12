import { invoke } from '@tauri-apps/api/core';
import { beforeEach, expect, it, vi } from 'vitest';

import { TauriLicensingPort } from './TauriLicensingPort';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset();
});

it('uses only the native licensing commands and passes a key only to activation', async () => {
  const port = new TauriLicensingPort();
  vi.mocked(invoke).mockResolvedValue(undefined);
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
