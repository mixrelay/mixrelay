import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { I18nProvider, i18n } from '../../core/i18n';
import type { RuntimePort, RuntimeStatus } from '../../core/runtime/RuntimePort';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { eventually, find } from '../../test-support/fakeTimerQueries';
import { RuntimeConnection } from './RuntimeConnection';

function renderConnection(port: RuntimePort, install = vi.fn()) {
  return render(
    <MixRelayProvider>
      <I18nProvider>
        <RuntimeConnection port={port} onInstall={install}>
          <label>
            Unsaved name
            <input defaultValue="Live routing" />
          </label>
        </RuntimeConnection>
      </I18nProvider>
    </MixRelayProvider>,
  );
}

describe('Driver connection gate', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en');
  });

  it('does not mount the editor until native readiness is acknowledged', async () => {
    let finish: (status: RuntimeStatus) => void = () => {};
    const connect = vi.fn(
      () =>
        new Promise<RuntimeStatus>((resolve) => {
          finish = resolve;
        }),
    );
    renderConnection({ connect, status: vi.fn() });
    expect(screen.getByRole('status')).toHaveTextContent('Connecting to MixRelay');
    expect(screen.getByRole('main')).toHaveAttribute('aria-busy', 'true');
    expect(screen.queryByRole('heading')).not.toBeInTheDocument();
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    await act(async () => {
      finish('ready');
    });
    expect(screen.getByRole('textbox')).toHaveValue('Live routing');
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it.each([
    ['not_installed', 'MixRelay is required', 'Install MixRelay'],
    ['incompatible', 'Update MixRelay', 'Update MixRelay'],
  ] as const)(
    'provides installation guidance for %s and only retries explicitly',
    async (status, title, action) => {
      const connect = vi
        .fn<RuntimePort['connect']>()
        .mockResolvedValueOnce(status)
        .mockResolvedValue('ready');
      const install = vi.fn();
      renderConnection({ connect, status: vi.fn() }, install);
      expect(await screen.findByRole('heading', { name: title })).toBeVisible();
      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: action }));
      expect(install).toHaveBeenCalledOnce();
      expect(connect).toHaveBeenCalledOnce();
      fireEvent.click(screen.getByRole('button', { name: 'Retry connection' }));
      expect(await screen.findByRole('textbox')).toBeVisible();
      expect(connect).toHaveBeenCalledTimes(2);
    },
  );

  it('ends a failed startup without exposing native errors', async () => {
    renderConnection({
      connect: vi.fn().mockRejectedValue(new Error('private socket error')),
      status: vi.fn(),
    });
    expect(
      await screen.findByRole('heading', { name: 'Cannot connect to MixRelay' }),
    ).toBeVisible();
    expect(screen.queryByText('private socket error')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Retry connection' })).toBeEnabled();
  });

  it('preserves editor state on disconnect and never automatically restarts stopped audio', async () => {
    // The two-second readiness re-check runs on the fake clock instead of wall time.
    vi.useFakeTimers();
    try {
      const connect = vi.fn<RuntimePort['connect']>().mockResolvedValue('ready');
      const status = vi.fn<RuntimePort['status']>().mockResolvedValue('unavailable');
      renderConnection({ connect, status });
      const input = await find('textbox');
      fireEvent.change(input, { target: { value: 'Unsaved edit' } });
      await eventually(
        () =>
          expect(screen.getByRole('heading', { name: 'Cannot connect to MixRelay' })).toBeVisible(),
        { timeout: 4000 },
      );
      expect(input).not.toBeVisible();
      expect(connect).toHaveBeenCalledOnce();
      fireEvent.click(screen.getByRole('button', { name: 'Retry connection' }));
      expect(await find('textbox')).toHaveValue('Unsaved edit');
    } finally {
      vi.useRealTimers();
    }
  });

  it('ignores a completed connection after unmount', async () => {
    let finish: (status: RuntimeStatus) => void = () => {};
    const view = renderConnection({
      connect: () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
      status: vi.fn(),
    });
    view.unmount();
    await act(async () => {
      finish('ready');
    });
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  });
});
