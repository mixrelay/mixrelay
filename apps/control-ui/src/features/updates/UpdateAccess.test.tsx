import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { I18nProvider, i18n } from '../../core/i18n';
import type { LicensingPort, LicensingStatus } from '../../core/licensing/LicensingPort';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import type { ProductUpdate, UpdatesPort } from '../../core/updates/UpdatesPort';
import { LicenseAccess } from '../licensing/LicenseAccess';
import { UpdateAccess } from './UpdateAccess';

const emptyProducts: ProductUpdate[] = [
  { product: 'mix-relay', installedVersion: '0.1.0', latestVersion: null,
    state: 'not_published', downloads: [], notesUrl: null },
  { product: 'mix-relay-driver', installedVersion: null, latestVersion: null,
    state: 'not_published', downloads: [], notesUrl: null },
];
const published: ProductUpdate[] = [
  { ...emptyProducts[0]!, latestVersion: '0.1.1', state: 'available',
    downloads: [{ os: 'macos', arch: 'universal', url: 'https://downloads.example/app.pkg',
      size: 12, sha256: 'a'.repeat(64) }] },
  { ...emptyProducts[1]!, latestVersion: '0.2.0', state: 'published',
    downloads: [{ os: 'linux', arch: 'x86_64', url: 'https://downloads.example/driver.deb',
      size: 12, sha256: 'b'.repeat(64) }] },
];

function port(check = vi.fn().mockResolvedValue(emptyProducts)): UpdatesPort {
  return { check, openDownload: vi.fn().mockResolvedValue(undefined) };
}

function view(native: UpdatesPort | null, licensingPort: LicensingPort | null = null) {
  return render(
    <MixRelayProvider>
      <I18nProvider>
        <UpdateAccess port={native}>
          {(action) => (
            <LicenseAccess additionalActions={action} port={licensingPort}>
              <header>MixRelay {action}</header>
            </LicenseAccess>
          )}
        </UpdateAccess>
      </I18nProvider>
    </MixRelayProvider>,
  );
}

beforeEach(async () => { await i18n.changeLanguage('en'); });

describe('update presentation', () => {
  it('has no browser entry or startup dialog', async () => {
    view(null);
    await act(async () => Promise.resolve());
    expect(screen.queryByRole('button', { name: 'Check for updates' })).not.toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('shows explicit empty catalogs as unpublished for both products, never current', async () => {
    const native = port();
    view(native);
    await waitFor(() => expect(native.check).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    const dialog = await screen.findByRole('dialog', { name: 'Updates' });
    expect(await within(dialog).findAllByText('No release has been published yet.')).toHaveLength(2);
    expect(within(dialog).queryByText('Up to date.')).not.toBeInTheDocument();
    expect(within(dialog).queryByRole('button', { name: /download/ })).not.toBeInTheDocument();
    expect(within(dialog).getByText('Installed: 0.1.0')).toBeInTheDocument();
    expect(within(dialog).getByText('Installed: Unknown')).toBeInTheDocument();
  });

  it('shows missing product entries as unavailable, never unpublished or current', async () => {
    view(port(vi.fn().mockResolvedValue([])));
    fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    const dialog = await screen.findByRole('dialog');
    expect(await within(dialog).findAllByText('Could not check this product. Try again when online.')).toHaveLength(2);
    expect(within(dialog).queryByText('No release has been published yet.')).not.toBeInTheDocument();
    expect(within(dialog).queryByText('Up to date.')).not.toBeInTheDocument();
  });

  it('shows current and unavailable states independently', async () => {
    const releases: ProductUpdate[] = [
      { ...emptyProducts[0]!, latestVersion: '0.1.0', state: 'current' },
      { ...emptyProducts[1]!, state: 'unavailable' },
    ];
    view(port(vi.fn().mockResolvedValue(releases)));
    fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    const dialog = await screen.findByRole('dialog');
    const app = within(dialog).getByRole('region', { name: 'MixRelay' });
    const driver = within(dialog).getByRole('region', { name: 'MixRelay Driver' });
    expect(await within(app).findByText('Up to date.')).toBeInTheDocument();
    expect(within(driver).getByText('Could not check this product. Try again when online.')).toBeInTheDocument();
    expect(within(driver).queryByText('Up to date.')).not.toBeInTheDocument();
  });

  it('indicates available releases without opening the dialog and opens the selected native asset', async () => {
    const native = port(vi.fn().mockResolvedValue(published));
    view(native);
    const action = await screen.findByRole('button', { name: 'Update available · Check for updates' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    fireEvent.click(action);
    const dialog = await screen.findByRole('dialog');
    const driver = within(dialog).getByRole('region', { name: 'MixRelay Driver' });
    expect(within(driver).getByText('Installed: Unknown')).toBeInTheDocument();
    expect(within(driver).getByText('Latest release: 0.2.0')).toBeInTheDocument();
    expect(within(driver).getByText('A release is available. The installed version could not be determined.')).toBeInTheDocument();
    expect(within(driver).queryByText('An update is available.')).not.toBeInTheDocument();
    const download = within(driver).getByRole('button', { name: 'Open DEB · x86_64 download' });
    await waitFor(() => expect(download).toBeEnabled());
    fireEvent.click(download);
    await waitFor(() => expect(native.openDownload).toHaveBeenCalledWith(
      'mix-relay-driver', 'https://downloads.example/driver.deb',
    ));
  });

  it('clears old download actions after refresh failure and retries without exposing native errors', async () => {
    const native = port(vi.fn().mockResolvedValueOnce(published).mockRejectedValueOnce('private URL')
      .mockResolvedValueOnce(emptyProducts));
    view(native);
    fireEvent.click(await screen.findByRole('button', { name: 'Update available · Check for updates' }));
    const dialog = await screen.findByRole('dialog');
    expect(await within(dialog).findAllByText('Could not check this product. Try again when online.')).toHaveLength(2);
    expect(within(dialog).queryByRole('button', { name: /download/ })).not.toBeInTheDocument();
    expect(within(dialog).queryByText('private URL')).not.toBeInTheDocument();
    expect(within(dialog).getByText('Installed: 0.1.0')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Check for updates' })).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Check again' }));
    expect(await within(dialog).findAllByText('No release has been published yet.')).toHaveLength(2);
  });

  it('shows a concise opening failure and leaves the asset retryable', async () => {
    const native = port(vi.fn().mockResolvedValue(published));
    vi.mocked(native.openDownload).mockRejectedValueOnce('private URL').mockResolvedValue(undefined);
    view(native);
    fireEvent.click(await screen.findByRole('button', { name: 'Update available · Check for updates' }));
    const dialog = await screen.findByRole('dialog');
    const download = within(dialog).getByRole('button', { name: 'Open PKG · universal download' });
    await waitFor(() => expect(download).toBeEnabled());
    fireEvent.click(download);
    expect(await within(dialog).findByRole('alert')).toHaveTextContent('Could not open the download.');
    await waitFor(() => expect(download).toBeEnabled());
    fireEvent.click(download);
    await waitFor(() => expect(native.openDownload).toHaveBeenCalledTimes(2));
    expect(within(dialog).queryByRole('alert')).not.toBeInTheDocument();
  });

  it('can close the dialog while a check is still pending', async () => {
    let resolve!: (value: ProductUpdate[]) => void;
    const native = port(vi.fn().mockReturnValue(new Promise<ProductUpdate[]>((yes) => { resolve = yes; })));
    view(native);
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByRole('status')).toHaveTextContent('Checking for updates');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(native.check).toHaveBeenCalledTimes(1);
    await act(async () => resolve(emptyProducts));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('provides a Korean accessible manual action', async () => {
    await i18n.changeLanguage('ko-KR');
    view(port());
    fireEvent.click(screen.getByRole('button', { name: '업데이트 확인' }));
    const dialog = await screen.findByRole('dialog', { name: '업데이트' });
    expect(await within(dialog).findAllByText('아직 게시된 릴리즈가 없습니다.')).toHaveLength(2);
  });
});

describe('update access through license presentation', () => {
  it.each(['expired', 'failure', 'licensed'])('keeps one usable update entry with %s licensing', async (status) => {
    const license: LicensingStatus = {
      status: status === 'expired' ? 'expired' : 'licensed',
      expiresAtUnix: 1_800_000_000, remainingDays: 10, configured: false,
    };
    const licensingPort: LicensingPort = {
      getStatus: status === 'failure' ? vi.fn().mockRejectedValue('unavailable') : vi.fn().mockResolvedValue(license),
      activate: vi.fn(), validate: vi.fn(), deactivate: vi.fn(), openCheckout: vi.fn(),
    };
    view(port(), licensingPort);
    if (status === 'failure') await screen.findByRole('alert');
    else if (status === 'expired') await screen.findByText(/Your access has ended/);
    else await screen.findByRole('button', { name: /Licensed/ });
    const entries = screen.getAllByRole('button', { name: 'Check for updates' });
    expect(entries).toHaveLength(1);
    fireEvent.click(entries[0]!);
    const dialog = await screen.findByRole('dialog', { name: 'Updates' });
    expect(await within(dialog).findAllByText('No release has been published yet.')).toHaveLength(2);
  });
});
