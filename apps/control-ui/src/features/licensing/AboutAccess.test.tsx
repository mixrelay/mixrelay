import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { version } from '../../../package.json';
import { I18nProvider, i18n } from '../../core/i18n';
import type { LicensingPort, LicensingStatus } from '../../core/licensing/LicensingPort';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { LicenseAccess } from './LicenseAccess';

const nativeMenu = vi.hoisted(() => ({ open: () => {} }));
vi.mock('../../core/app-info/useAppInformation', () => ({
  useAppInformation: (open: () => void) => {
    nativeMenu.open = open;
    return version;
  },
}));

function view(port: LicensingPort | null) {
  return render(
    <MixRelayProvider>
      <I18nProvider>
        <LicenseAccess port={port} presentation="footer">
          {(action) => (
            <main>
              <p>Workspace</p>
              <footer>{action}</footer>
            </main>
          )}
        </LicenseAccess>
      </I18nProvider>
    </MixRelayProvider>,
  );
}

it('opens only authentication from the version and confirmed license status', async () => {
  await i18n.changeLanguage('en');
  const port: LicensingPort = {
    getStatus: vi.fn().mockResolvedValue({
      status: 'free',
      configured: false,
      remainingDays: 0,
      expiresAtUnix: null,
    }),
    activate: vi.fn(),
    validate: vi.fn(),
    deactivate: vi.fn(),
    openCheckout: vi.fn(),
  };
  view(port);
  const trigger = screen.getByRole('button', { name: 'Licence' });
  await waitFor(() => expect(trigger).toHaveTextContent(`${version} · Free`));
  expect(trigger).toHaveAttribute('aria-description', `${version} · Free`);
  expect(trigger).not.toHaveAttribute('title');
  expect(trigger.closest('footer')).not.toBeNull();
  expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
  trigger.focus();
  fireEvent.click(trigger);
  const dialog = await screen.findByRole('dialog', { name: 'Licence' });
  expect(within(dialog).getByText(i18n.t('licensing.freeSummary'))).toBeVisible();
  expect(within(dialog).queryByText('Terms of use')).not.toBeInTheDocument();
  expect(within(dialog).queryByRole('button', { name: 'Stop audio' })).not.toBeInTheDocument();
  fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
  await waitFor(() => expect(trigger).toHaveFocus());
});

it('can open authentication without a licensing service and does not claim free status', async () => {
  await i18n.changeLanguage('en');
  view(null);
  const trigger = screen.getByRole('button', { name: 'Licence' });
  expect(trigger).toHaveTextContent('Licence status unavailable');
  expect(trigger).not.toHaveTextContent('Free');
  fireEvent.click(trigger);
  const dialog = await screen.findByRole('dialog', { name: 'Licence' });
  await waitFor(() => expect(within(dialog).getByText('Licence status unavailable')).toBeVisible());
  expect(within(dialog).queryByRole('button', { name: 'Activate' })).not.toBeInTheDocument();
});

it('keeps product information and the localized contract in native About, separate from license actions', async () => {
  await i18n.changeLanguage('en');
  view(null);
  act(() => nativeMenu.open());
  const dialog = await screen.findByRole('dialog', { name: 'About MixRelay' });
  expect(within(dialog).getByText(`Version ${version}`)).toBeVisible();
  expect(within(dialog).getByText('Echo Vision Lab')).toBeVisible();
  expect(within(dialog).getByRole('link', { name: 'support@rawsector.com' })).toBeVisible();
  const disclosure = dialog.querySelector('details');
  expect(disclosure).not.toHaveAttribute('open');
  expect(disclosure?.querySelector('pre')).not.toBeVisible();
  expect(within(dialog).queryByText('Licence status unavailable')).not.toBeInTheDocument();
  expect(within(dialog).queryByRole('button', { name: 'Activate' })).not.toBeInTheDocument();
  expect(within(dialog).queryByRole('button', { name: 'Stop audio' })).not.toBeInTheDocument();
});

it('defers native About while licensing is busy and closes About without hiding the workspace', async () => {
  await i18n.changeLanguage('en');
  const status: LicensingStatus = {
    status: 'licensed',
    configured: true,
    remainingDays: 1,
    expiresAtUnix: null,
  };
  const pending = Promise.withResolvers<LicensingStatus>();
  const port: LicensingPort = {
    getStatus: vi.fn().mockResolvedValue(status),
    activate: vi.fn(),
    validate: vi.fn(() => pending.promise),
    deactivate: vi.fn(),
    openCheckout: vi.fn(),
  };
  view(port);
  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Licence' })).toHaveTextContent('Licensed'),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Licence' }));
  const dialog = await screen.findByRole('dialog', { name: 'Licence' });
  fireEvent.click(within(dialog).getByRole('button', { name: 'Check licence online' }));
  await waitFor(() =>
    expect(within(dialog).getByRole('button', { name: 'Check licence online' })).toBeDisabled(),
  );
  act(() => nativeMenu.open());
  expect(screen.queryByRole('dialog', { name: 'About MixRelay' })).not.toBeInTheDocument();
  await act(async () => pending.resolve(status));
  act(() => nativeMenu.open());
  const about = await screen.findByRole('dialog', { name: 'About MixRelay' });
  fireEvent.click(within(about).getByRole('button', { name: 'Close' }));
  await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  expect(screen.getByText('Workspace')).toBeInTheDocument();
});
