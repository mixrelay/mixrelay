import { expect, it, vi } from 'vitest';
import { TauriLicensingPort } from './TauriLicensingPort';

const free = { status: 'free', expiresAtUnix: null, remainingDays: 0, configured: true };

it('uses only the native licensing commands and passes a key only to activation', async () => {
  const invoke = vi.fn().mockResolvedValue(free);
  const port = new TauriLicensingPort(invoke);
  await port.getStatus();
  await port.activate('secret-license-key', 'Studio Mac');
  await port.validate();
  await port.deactivate();
  await port.openCheckout();
  expect(invoke.mock.calls).toEqual([
    ['licensing_status'],
    ['licensing_activate', { key: 'secret-license-key', instanceName: 'Studio Mac' }],
    ['licensing_validate'],
    ['licensing_deactivate'],
    ['licensing_open_checkout'],
  ]);
});

it('rejects a status that is not a licensing state, including an unknown access state', async () => {
  const invoke = vi.fn();
  const port = new TauriLicensingPort(invoke);
  for (const reply of [undefined, { ...free, status: 'trial' }, { ...free, remainingDays: -1 }]) {
    invoke.mockResolvedValueOnce(reply);
    await expect(port.getStatus()).rejects.toThrow('invalid response at licensing_status');
  }
  invoke.mockResolvedValueOnce({
    ...free,
    status: 'licensed',
    expiresAtUnix: 9,
    hasActivation: true,
  });
  await expect(port.validate()).resolves.toMatchObject({ status: 'licensed', hasActivation: true });
});
