import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import type { LicensingStatus } from '../../core/domain/licensing';
import { I18nProvider, i18n } from '../../core/i18n';
import type { LicensingPort } from '../../core/ports/LicensingPort';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { LicenseDetails } from './LicenseDetails';
import type { LicenseErrorCode } from './useLicenseSession';

const free: LicensingStatus = {
  status: 'free',
  configured: true,
  remainingDays: 0,
  expiresAtUnix: null,
};

function view(status = free, success = true, busy = false, error: LicenseErrorCode | null = null) {
  const port: LicensingPort = {
    getStatus: vi.fn().mockResolvedValue(free),
    activate: vi.fn().mockResolvedValue(free),
    validate: vi.fn().mockResolvedValue(free),
    deactivate: vi.fn().mockResolvedValue(free),
    openCheckout: vi.fn().mockResolvedValue(undefined),
  };
  const onMutated = vi.fn();
  const run = vi.fn(async (action: () => Promise<LicensingStatus> | Promise<void>) => {
    if (success) await action();
    return success;
  });
  render(
    <MixRelayProvider>
      <I18nProvider>
        <LicenseDetails
          busy={busy}
          error={error}
          onMutated={onMutated}
          port={port}
          run={run}
          status={status}
        />
      </I18nProvider>
    </MixRelayProvider>,
  );
  return { port, run, onMutated };
}

beforeEach(async () => {
  await i18n.changeLanguage('en');
});

it.each([null, Number.NaN, Number.POSITIVE_INFINITY, 1e30])(
  'omits invalid expiry %s while keeping paid controls available',
  (expiresAtUnix) => {
    const f = view({ ...free, status: 'licensed', expiresAtUnix });
    expect(screen.queryByText(/Valid until/u)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Deactivate this device' })).toBeEnabled();
    expect(screen.queryByLabelText('Licence key')).not.toBeInTheDocument();
    expect(f.run).not.toHaveBeenCalled();
  },
);

it.each(['key', 'name', 'busy'] as const)(
  'rejects an incomplete or busy activation submission: %s',
  (reason) => {
    const f = view(free, true, reason === 'busy');
    const key = screen.getByLabelText('Licence key');
    const name = screen.getByRole('textbox', { name: 'Device name' });
    if (reason !== 'key') fireEvent.change(key, { target: { value: 'license-key' } });
    if (reason !== 'name') fireEvent.change(name, { target: { value: 'Studio Mac' } });
    const form = key.closest('form');
    if (!form) throw new Error('Activation fields need their native form owner');
    fireEvent.submit(form);
    expect(f.run).not.toHaveBeenCalled();
    expect(f.port.activate).not.toHaveBeenCalled();
  },
);

it('keeps the entered key and device name when activation fails', async () => {
  const f = view(free, false);
  const key = screen.getByLabelText('Licence key');
  fireEvent.change(key, { target: { value: 'license-key' } });
  fireEvent.change(screen.getByRole('textbox', { name: 'Device name' }), {
    target: { value: 'Studio Mac' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Activate' }));
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(key).toHaveValue('license-key');
  expect(screen.getByRole('textbox', { name: 'Device name' })).toHaveValue('Studio Mac');
  expect(f.onMutated).not.toHaveBeenCalled();
});

it('retains the paid panel when deactivation is not acknowledged', async () => {
  const f = view({ ...free, status: 'licensed' }, false);
  fireEvent.click(screen.getByRole('button', { name: 'Deactivate this device' }));
  await vi.waitFor(() => expect(f.run).toHaveBeenCalledTimes(1));
  expect(f.onMutated).not.toHaveBeenCalled();
  expect(screen.getByRole('button', { name: 'Deactivate this device' })).toBeInTheDocument();
});

it('keeps the free graph capabilities visible when an activation request fails', () => {
  view(free, false, false, 'unavailable');
  expect(screen.getByRole('alert')).toHaveTextContent('The licensing service is unavailable');
  expect(screen.getByText(/You can save and load the free graph/u)).toBeInTheDocument();
  expect(screen.getByLabelText('Licence key')).toBeEnabled();
});
