import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import licenseTerms from '../../../../../LICENSE.txt?raw';
import { I18nProvider, i18n } from '../../core/i18n';
import type { LicensingPort, LicensingStatus } from '../../core/licensing/LicensingPort';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { LicenseAccess } from './LicenseAccess';

const trial: LicensingStatus = {
  status: 'trial',
  expiresAtUnix: 1_800_000_000,
  remainingDays: 8,
  configured: true,
};
const licensed: LicensingStatus = { ...trial, status: 'licensed', remainingDays: 27 };
const expired: LicensingStatus = {
  ...trial,
  status: 'expired',
  expiresAtUnix: 1_600_000_000,
  remainingDays: 0,
};

function port(status: LicensingStatus = trial): LicensingPort {
  return {
    getStatus: vi.fn().mockResolvedValue(status),
    activate: vi.fn().mockResolvedValue(licensed),
    validate: vi.fn().mockResolvedValue(licensed),
    deactivate: vi.fn().mockResolvedValue(expired),
    openCheckout: vi.fn().mockResolvedValue(undefined),
  };
}

function view(
  licensingPort: LicensingPort | null,
  children: ReactNode = <div>Live controls</div>,
  onStopAudio?: () => Promise<void>,
) {
  return render(
    <MixRelayProvider>
      <I18nProvider>
        <LicenseAccess onStopAudio={onStopAudio} port={licensingPort}>
          {children}
        </LicenseAccess>
      </I18nProvider>
    </MixRelayProvider>,
  );
}

beforeEach(async () => {
  await i18n.changeLanguage('en');
});

afterEach(() => vi.useRealTimers());

describe('LicenseAccess display', () => {
  it('leaves browser preview unchanged without asserting a local entitlement', () => {
    view(null);
    expect(screen.getByText('Live controls')).toBeInTheDocument();
    expect(screen.queryByText(/Trial|Licensed/u)).not.toBeInTheDocument();
  });

  it('shows a small loading state, then a compact trial entry and details on demand', async () => {
    let finish!: (value: LicensingStatus) => void;
    const licensingPort = port();
    vi.mocked(licensingPort.getStatus).mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    view(licensingPort);
    expect(screen.getByRole('status')).toHaveTextContent('Checking license');
    expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
    finish(trial);
    const entry = await screen.findByRole('button', { name: 'Trial · 8 days left' });
    expect(screen.getByText('Live controls')).toBeInTheDocument();
    fireEvent.click(entry);
    const dialog = await screen.findByRole('dialog', { name: 'License' });
    expect(within(dialog).getByText(/Valid until/u)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Buy a license' }));
    expect(licensingPort.openCheckout).toHaveBeenCalledTimes(1);
    expect(licensingPort.getStatus).toHaveBeenCalledTimes(1);
  });

  it('gates expired access and activates through a password field without retaining the entered key', async () => {
    const licensingPort = port(expired);
    view(licensingPort);
    expect(await screen.findByText(/Your access has ended/u)).toBeInTheDocument();
    expect(screen.getByText(/Expired on/u)).toBeInTheDocument();
    expect(screen.queryByText(/Valid until/u)).not.toBeInTheDocument();
    expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
    const key = screen.getByLabelText('License key');
    expect(key).toHaveAttribute('type', 'password');
    fireEvent.change(key, { target: { value: 'secret-license-key' } });
    fireEvent.change(screen.getByRole('textbox', { name: 'Device name' }), {
      target: { value: 'Studio Mac' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Activate' }));
    await waitFor(() =>
      expect(licensingPort.activate).toHaveBeenCalledWith('secret-license-key', 'Studio Mac'),
    );
    expect(await screen.findByText('Live controls')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /Licensed/u }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).queryByLabelText('License key')).not.toBeInTheDocument();
    expect(within(dialog).queryByRole('button', { name: 'Buy a license' })).not.toBeInTheDocument();
    expect(within(dialog).getByText(/Valid until/u)).toBeInTheDocument();
    expect(
      within(dialog).getByRole('button', { name: 'Check license online' }),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole('button', { name: 'Deactivate this device' }),
    ).toBeInTheDocument();
  });

  it('shows clock repair without live controls', async () => {
    view(port({ ...expired, status: 'clock_invalid' }));
    expect(await screen.findByText(/Check your device clock/u)).toBeInTheDocument();
    expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
    expect(screen.queryByLabelText('License key')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Check again' })).toBeInTheDocument();
  });

  it('does not offer purchase or activation in an unconfigured build', async () => {
    const licensingPort = port({ ...expired, configured: false });
    view(licensingPort);
    expect(
      await screen.findByText('Purchases and activation are not available in this build.'),
    ).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Buy a license' })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Activate' })).not.toBeInTheDocument();
    expect(licensingPort.openCheckout).not.toHaveBeenCalled();
  });

  it('refreshes native status after one minute and stops automatic checks after an error', async () => {
    vi.useFakeTimers();
    const licensingPort = port(trial);
    vi.mocked(licensingPort.getStatus)
      .mockResolvedValueOnce(trial)
      .mockRejectedValueOnce('storage_unavailable');
    view(licensingPort);
    await act(async () => Promise.resolve());
    expect(screen.getByRole('button', { name: 'Trial · 8 days left' })).toBeInTheDocument();
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(screen.getByRole('alert')).toHaveTextContent('License storage is unavailable');
    expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
    await act(async () => vi.advanceTimersByTimeAsync(120_000));
    expect(licensingPort.getStatus).toHaveBeenCalledTimes(2);
  });
});

it('keeps terms and product-integration contact available without configured or active access', async () => {
  view(port({ ...expired, configured: false }));
  expect(
    await screen.findByText('Purchases and activation are not available in this build.'),
  ).toBeInTheDocument();
  expect(
    screen.getByText(/Product integration requires a separate paid agreement/u),
  ).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'support@mixrelay.com' })).toHaveAttribute(
    'href',
    'mailto:support@mixrelay.com',
  );
  const terms = screen.getByText('Terms of use').closest('details');
  expect(terms).toBeInTheDocument();
  expect(terms).not.toHaveAttribute('open');
  expect(terms?.querySelector('pre')).toHaveTextContent(
    'MixRelay App Software License Agreement',
  );
  expect(terms?.querySelector('pre')?.textContent).toBe(licenseTerms);
});

describe('LicenseAccess operations', () => {
  it('localizes failures without displaying raw errors or automatically retrying', async () => {
    const licensingPort = port();
    vi.mocked(licensingPort.getStatus).mockRejectedValueOnce(new Error('secret-license-key'));
    view(licensingPort);
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not check the license');
    expect(screen.queryByText('secret-license-key')).not.toBeInTheDocument();
    expect(licensingPort.getStatus).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'Check again' }));
    expect(await screen.findByText('Live controls')).toBeInTheDocument();
    expect(licensingPort.getStatus).toHaveBeenCalledTimes(2);
  });

  it('guards duplicate actions and supports explicit validation and deactivation', async () => {
    let finish!: (value: LicensingStatus) => void;
    const licensingPort = port(licensed);
    vi.mocked(licensingPort.validate).mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    view(licensingPort);
    fireEvent.click(await screen.findByRole('button', { name: /Licensed/u }));
    const dialog = await screen.findByRole('dialog');
    const validate = within(dialog).getByRole('button', { name: 'Check license online' });
    fireEvent.click(validate);
    fireEvent.click(validate);
    expect(licensingPort.validate).toHaveBeenCalledTimes(1);
    expect(validate).toBeDisabled();
    finish(licensed);
    await waitFor(() => expect(validate).toBeEnabled());
    fireEvent.click(within(dialog).getByRole('button', { name: 'Deactivate this device' }));
    expect(await screen.findByText(/Your access has ended/u)).toBeInTheDocument();
    expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
    expect(licensingPort.deactivate).toHaveBeenCalledTimes(1);
  });

  it.each(['rejected', 'wrong_product'])(
    'removes stale licensed controls after a definitive %s validation error',
    async (code) => {
      const licensingPort = port(licensed);
      vi.mocked(licensingPort.getStatus)
        .mockResolvedValueOnce(licensed)
        .mockResolvedValueOnce(expired);
      vi.mocked(licensingPort.validate).mockRejectedValueOnce(code);
      view(licensingPort);
      fireEvent.click(await screen.findByRole('button', { name: /Licensed/u }));
      const dialog = await screen.findByRole('dialog');
      fireEvent.click(within(dialog).getByRole('button', { name: 'Check license online' }));
      expect(await screen.findByText(/Your access has ended/u)).toBeInTheDocument();
      expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
      expect(licensingPort.getStatus).toHaveBeenCalledTimes(2);
      expect(screen.getByRole('alert')).toBeInTheDocument();
    },
  );

  it('keeps the local entitlement on a nondefinitive request failure', async () => {
    const licensingPort = port(licensed);
    vi.mocked(licensingPort.validate).mockRejectedValueOnce('request_failed');
    view(licensingPort);
    fireEvent.click(await screen.findByRole('button', { name: /Licensed/u }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Check license online' }));
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('license request failed');
    expect(screen.getByText('Live controls')).toBeInTheDocument();
    expect(licensingPort.getStatus).toHaveBeenCalledTimes(1);
  });

  it('shows a safe known error after activation fails', async () => {
    const licensingPort = port(expired);
    vi.mocked(licensingPort.getStatus)
      .mockResolvedValueOnce(expired)
      .mockResolvedValueOnce(licensed);
    vi.mocked(licensingPort.activate).mockRejectedValueOnce('storage_unavailable');
    view(licensingPort);
    const key = await screen.findByLabelText('License key');
    fireEvent.change(key, { target: { value: 'secret-license-key' } });
    fireEvent.change(screen.getByRole('textbox', { name: 'Device name' }), {
      target: { value: 'Studio Mac' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Activate' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('License storage is unavailable');
    expect(screen.queryByText('secret-license-key')).not.toBeInTheDocument();
    expect(screen.getByText('Live controls')).toBeInTheDocument();
    expect(licensingPort.getStatus).toHaveBeenCalledTimes(2);
  });
});

describe('LicenseAccess storage recovery', () => {
  it('shows the refreshed state after deactivation reports a storage failure', async () => {
    const licensingPort = port(licensed);
    vi.mocked(licensingPort.getStatus)
      .mockResolvedValueOnce(licensed)
      .mockResolvedValueOnce(expired);
    vi.mocked(licensingPort.deactivate).mockRejectedValueOnce('storage_unavailable');
    view(licensingPort);
    fireEvent.click(await screen.findByRole('button', { name: /Licensed/u }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Deactivate this device' }));
    expect(await screen.findByText(/Your access has ended/u)).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('License storage is unavailable');
    expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
    expect(licensingPort.getStatus).toHaveBeenCalledTimes(2);
  });

  it('hides stale licensed controls if storage reconciliation also fails', async () => {
    const licensingPort = port(licensed);
    vi.mocked(licensingPort.getStatus)
      .mockResolvedValueOnce(licensed)
      .mockRejectedValueOnce('storage_unavailable');
    vi.mocked(licensingPort.deactivate).mockRejectedValueOnce('storage_unavailable');
    view(licensingPort);
    fireEvent.click(await screen.findByRole('button', { name: /Licensed/u }));
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Deactivate this device' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('License storage is unavailable');
    expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
    expect(licensingPort.getStatus).toHaveBeenCalledTimes(2);
  });
});

describe('LicenseAccess audio stop action', () => {
  it('keeps a failed stop request separate from licensing and hides raw errors', async () => {
    const onStopAudio = vi.fn().mockRejectedValue(new Error('secret device failure'));
    view(port(expired), undefined, onStopAudio);
    fireEvent.click(await screen.findByRole('button', { name: 'Stop audio' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Could not stop audio');
    expect(screen.queryByText('secret device failure')).not.toBeInTheDocument();
    expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
    expect(onStopAudio).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: 'Stop audio' })).toBeEnabled();
  });

  it('guards a second stop click while the first request is pending', async () => {
    let finish!: () => void;
    const pending = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const onStopAudio = vi.fn().mockReturnValue(pending);
    view(port(expired), undefined, onStopAudio);
    const button = await screen.findByRole('button', { name: 'Stop audio' });
    act(() => {
      fireEvent.click(button);
      fireEvent.click(button);
    });
    expect(onStopAudio).toHaveBeenCalledTimes(1);
    expect(button).toBeDisabled();
    await act(async () => finish());
    expect(screen.getByRole('status')).toHaveTextContent('Stop request sent');
  });
});

describe('LicenseAccess edge states', () => {
  it('offers audio stop when the initial license read fails', async () => {
    const licensingPort = port();
    vi.mocked(licensingPort.getStatus).mockRejectedValueOnce('storage_unavailable');
    const onStopAudio = vi.fn().mockResolvedValue(undefined);
    view(licensingPort, undefined, onStopAudio);
    expect(await screen.findByRole('alert')).toHaveTextContent('License storage is unavailable');
    expect(screen.getByText('Terms of use').closest('details')).not.toHaveAttribute('open');
    expect(screen.getByRole('link', { name: 'support@mixrelay.com' })).toHaveAttribute(
      'href',
      'mailto:support@mixrelay.com',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Stop audio' }));
    await waitFor(() => expect(onStopAudio).toHaveBeenCalledTimes(1));
    expect(screen.queryByText('Live controls')).not.toBeInTheDocument();
  });

  it('closes the active license dialog and returns focus to its entry', async () => {
    view(port(trial));
    const entry = await screen.findByRole('button', { name: /Trial/u });
    fireEvent.click(entry);
    const dialog = await screen.findByRole('dialog');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(entry).toHaveAttribute('aria-expanded', 'false');
  });

  it('rechecks a corrected device clock without offering activation', async () => {
    const licensingPort = port({ ...expired, status: 'clock_invalid' });
    vi.mocked(licensingPort.getStatus)
      .mockResolvedValueOnce({ ...expired, status: 'clock_invalid' })
      .mockResolvedValueOnce(trial);
    view(licensingPort);
    fireEvent.click(await screen.findByRole('button', { name: 'Check again' }));
    expect(await screen.findByText('Live controls')).toBeInTheDocument();
    expect(licensingPort.getStatus).toHaveBeenCalledTimes(2);
    expect(licensingPort.activate).not.toHaveBeenCalled();
  });

  it('does not activate from an empty submitted form', async () => {
    const licensingPort = port(expired);
    view(licensingPort);
    const activate = await screen.findByRole('button', { name: 'Activate' });
    const form = activate.closest('form');
    if (!form) throw new Error('Activation form missing');
    fireEvent.submit(form);
    expect(licensingPort.activate).not.toHaveBeenCalled();
  });

  it('does not render an invalid expiry and formats a valid expiry in Korean', async () => {
    const first = view(port({ ...trial, expiresAtUnix: Number.MAX_VALUE }));
    fireEvent.click(await screen.findByRole('button', { name: /Trial/u }));
    expect(
      within(await screen.findByRole('dialog')).queryByText(/Valid until/u),
    ).not.toBeInTheDocument();
    first.unmount();

    await i18n.changeLanguage('ko-KR');
    view(port(trial));
    fireEvent.click(await screen.findByRole('button', { name: /평가판/u }));
    expect(within(await screen.findByRole('dialog')).getByText(/까지 유효/u)).toBeInTheDocument();
  });
});
