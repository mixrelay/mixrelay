import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { I18nProvider, i18n } from '../../core/i18n';
import type { LicensingPort, LicensingStatus } from '../../core/licensing/LicensingPort';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { LicenseAccess } from './LicenseAccess';

const free: LicensingStatus = {
  status: 'free',
  expiresAtUnix: null,
  remainingDays: 0,
  configured: true,
  hasActivation: false,
};
const licensed: LicensingStatus = {
  ...free,
  status: 'licensed',
  expiresAtUnix: 1_800_000_000,
  remainingDays: 27,
  hasActivation: true,
};
function port(status = free): LicensingPort {
  return {
    getStatus: vi.fn().mockResolvedValue(status),
    activate: vi.fn().mockResolvedValue(licensed),
    validate: vi.fn().mockResolvedValue(licensed),
    deactivate: vi.fn().mockResolvedValue(free),
    openCheckout: vi.fn().mockResolvedValue(undefined),
  };
}
function view(licensingPort: LicensingPort | null, onPaidAccessChange = vi.fn()) {
  return render(
    <MixRelayProvider>
      <I18nProvider>
        <LicenseAccess port={licensingPort} onPaidAccessChange={onPaidAccessChange}>
          <div>Live controls</div>
        </LicenseAccess>
      </I18nProvider>
    </MixRelayProvider>,
  );
}
async function open(label: RegExp | string = 'Free') {
  fireEvent.click(await screen.findByRole('button', { name: label }));
  return within(await screen.findByRole('dialog', { name: 'License' }));
}
beforeEach(async () => {
  await i18n.changeLanguage('en');
});
afterEach(() => vi.useRealTimers());

it('keeps controls available while checking, with unlimited free access and no trial expiry', async () => {
  let finish!: (value: LicensingStatus) => void;
  const native = port();
  vi.mocked(native.getStatus).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  view(native);
  expect(screen.getByText('Live controls')).toBeInTheDocument();
  await act(async () => finish(free));
  const dialog = await open();
  expect(dialog.getByText(/Free: 1 graph/u)).toBeInTheDocument();
  expect(
    dialog.getByText(/You can save and load the free graph and its channel mappings/u),
  ).toBeInTheDocument();
  expect(dialog.queryByText(/Trial|Valid until|Expired on/u)).not.toBeInTheDocument();
  fireEvent.click(dialog.getByRole('button', { name: 'Buy a license' }));
  expect(native.openCheckout).toHaveBeenCalledTimes(1);
});

it('activates through a password field and clears the form without interrupting audio', async () => {
  const native = port();
  view(native);
  const dialog = await open();
  expect(dialog.getByLabelText('License key')).toHaveAttribute('type', 'password');
  fireEvent.change(dialog.getByLabelText('License key'), { target: { value: 'test-key' } });
  fireEvent.change(dialog.getByRole('textbox', { name: 'Device name' }), {
    target: { value: 'Studio Mac' },
  });
  fireEvent.click(dialog.getByRole('button', { name: 'Activate' }));
  await waitFor(() => expect(native.activate).toHaveBeenCalledWith('test-key', 'Studio Mac'));
  const paid = await open(/Licensed/u);
  expect(paid.queryByLabelText('License key')).not.toBeInTheDocument();
  expect(paid.getByText(/Valid until/u)).toBeInTheDocument();
});

it('renews an expired saved seat without allocating another activation', async () => {
  const native = port({ ...free, hasActivation: true });
  view(native);
  const dialog = await open();
  expect(dialog.queryByLabelText('License key')).not.toBeInTheDocument();
  fireEvent.click(dialog.getByRole('button', { name: 'Check license online' }));
  await waitFor(() => expect(native.validate).toHaveBeenCalledTimes(1));
  expect(native.activate).not.toHaveBeenCalled();
  expect(screen.getByText('Live controls')).toBeInTheDocument();
});

it('retains running controls on lease expiry, clock rollback and storage errors', async () => {
  vi.useFakeTimers();
  const native = port(licensed);
  const paid = vi.fn();
  vi.mocked(native.getStatus)
    .mockResolvedValueOnce(licensed)
    .mockResolvedValueOnce({ ...free, hasActivation: true })
    .mockResolvedValueOnce({ ...free, status: 'clock_invalid' })
    .mockRejectedValueOnce('storage_unavailable');
  view(native, paid);
  await act(async () => Promise.resolve());
  expect(paid).toHaveBeenLastCalledWith(true);
  await act(async () => vi.advanceTimersByTimeAsync(60_000));
  expect(paid).toHaveBeenLastCalledWith(false);
  await act(async () => vi.advanceTimersByTimeAsync(120_000));
  expect(screen.getByText('Live controls')).toBeInTheDocument();
  expect(screen.getByRole('alert')).toHaveTextContent('License storage is unavailable');
  expect(native.getStatus).toHaveBeenCalledTimes(4);
  // A failed check is retried soon instead of waiting for the next regular check.
  await act(async () => vi.advanceTimersByTimeAsync(2_000));
  expect(native.getStatus).toHaveBeenCalledTimes(5);
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  expect(paid).toHaveBeenLastCalledWith(true);
});

it('offers clock correction without disabling free audio', async () => {
  const native = port({ ...free, status: 'clock_invalid' });
  view(native);
  const dialog = await open('Device clock needs attention');
  expect(dialog.queryByLabelText('License key')).not.toBeInTheDocument();
  fireEvent.click(dialog.getByRole('button', { name: 'Check again' }));
  await waitFor(() => expect(native.getStatus).toHaveBeenCalledTimes(2));
  expect(screen.getByText('Live controls')).toBeInTheDocument();
});

it('keeps license limits available without mixing legal information into authentication', async () => {
  const native = port({ ...free, configured: false });
  view(native);
  const dialog = await open();
  expect(
    dialog.getByText('Purchases and activation are not available in this build.'),
  ).toBeInTheDocument();
  expect(dialog.queryByText('Terms of use')).not.toBeInTheDocument();
  expect(dialog.queryByRole('button', { name: 'Stop audio' })).not.toBeInTheDocument();
  expect(dialog.queryByRole('button', { name: 'Activate' })).not.toBeInTheDocument();
});

it('hides raw failures, supports explicit retry and guards pending license actions', async () => {
  vi.useFakeTimers();
  const native = port();
  vi.mocked(native.getStatus)
    .mockRejectedValueOnce(new Error('secret-key'))
    .mockRejectedValueOnce(new Error('secret-key'))
    .mockRejectedValueOnce(new Error('secret-key'));
  view(native);
  await act(async () => Promise.resolve());
  // An unreached check (the runtime may still be starting) is retried before it is shown.
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  await act(async () => vi.advanceTimersByTimeAsync(6_000));
  expect(native.getStatus).toHaveBeenCalledTimes(3);
  expect(screen.getByRole('alert')).toHaveTextContent('Could not check the license');
  expect(screen.queryByText('secret-key')).not.toBeInTheDocument();
  vi.useRealTimers();
  const dialog = await open();
  fireEvent.click(dialog.getByRole('button', { name: 'Check again' }));
  await waitFor(() => expect(native.getStatus).toHaveBeenCalledTimes(4));
  expect(screen.getByText('Live controls')).toBeInTheDocument();
});

it('deactivation preserves live controls and returns to the free plan', async () => {
  const native = port(licensed);
  view(native);
  const dialog = await open(/Licensed/u);
  fireEvent.click(dialog.getByRole('button', { name: 'Deactivate this device' }));
  expect(await screen.findByRole('button', { name: 'Free' })).toBeInTheDocument();
  expect(screen.getByText('Live controls')).toBeInTheDocument();
});

it('passes no license action to a browser-only functional workspace', () => {
  const children = vi.fn(() => <div>Browser workspace</div>);
  render(<LicenseAccess port={null}>{children}</LicenseAccess>);
  expect(children).toHaveBeenCalledWith(null);
  expect(screen.getByText('Browser workspace')).toBeInTheDocument();
  expect(screen.queryByRole('button')).not.toBeInTheDocument();
});

it('keeps an open initial check visible and closes the license without interrupting controls', async () => {
  const pending = Promise.withResolvers<LicensingStatus>();
  const native = port();
  vi.mocked(native.getStatus).mockReturnValue(pending.promise);
  view(native);
  const dialog = await open();
  expect(dialog.getByRole('status')).toHaveTextContent('Checking license');
  fireEvent.click(dialog.getByRole('button', { name: 'Close' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  await act(async () => pending.resolve(free));
  expect(screen.getByText('Live controls')).toBeInTheDocument();
});

it('revokes presentation access when the native licensing owner is replaced', async () => {
  const paid = vi.fn();
  const native = port(licensed);
  const component = (value: LicensingPort | null) => (
    <MixRelayProvider>
      <I18nProvider>
        <LicenseAccess port={value} onPaidAccessChange={paid}>
          <div>Live controls</div>
        </LicenseAccess>
      </I18nProvider>
    </MixRelayProvider>
  );
  const rendered = render(component(native));
  await waitFor(() => expect(paid).toHaveBeenLastCalledWith(true));
  rendered.rerender(component(null));
  expect(paid).toHaveBeenLastCalledWith(false);
  expect(screen.getByText('Live controls')).toBeInTheDocument();
  expect(screen.queryByRole('button')).not.toBeInTheDocument();
});

it('shows a failed initial check inside the already open license dialog and supports retry', async () => {
  const pending = Promise.withResolvers<LicensingStatus>();
  const native = port();
  vi.mocked(native.getStatus).mockReturnValueOnce(pending.promise);
  view(native);
  const dialog = await open();
  await act(async () => pending.reject('unavailable'));
  expect(dialog.getByRole('alert')).toHaveTextContent('The licensing service is unavailable');
  fireEvent.click(dialog.getByRole('button', { name: 'Check again' }));
  await waitFor(() => expect(dialog.getByLabelText('License key')).toBeEnabled());
  expect(screen.getByText('Live controls')).toBeInTheDocument();
});
