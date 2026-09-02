import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { StorybookMockControlPort } from '../core/control-port/StorybookMockControlPort';
import type { ToneNariStatus } from '../core/control-port/types';
import { I18nProvider, i18n, resolveLocale } from '../core/i18n';
import { ToneNariProvider } from '../core/theme/ToneNariProvider';
import { App } from './App';

describe('App composition', () => {
  it('renders the Core to Feature to Screen composition', async () => {
    await i18n.changeLanguage('en');
    render(
      <ToneNariProvider>
        <I18nProvider>
          <App />
        </I18nProvider>
      </ToneNariProvider>,
    );

    expect(
      screen.getByRole('heading', { name: 'Route sound without breaking your flow.' }),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Install driver' })).toBeEnabled();
    expect(screen.getByText('Remote control')).toBeInTheDocument();
  });

  it('resolves supported locales and returns mock control status', async () => {
    expect(resolveLocale('ko-KR')).toBe('ko-KR');
    expect(resolveLocale('en-US')).toBe('en');

    const status: ToneNariStatus = {
      bufferFrames: null,
      driverState: 'not_installed',
      overruns: 0,
      protocolVersion: 1,
      sampleRate: null,
      underruns: 0,
    };
    await expect(new StorybookMockControlPort(status).getStatus()).resolves.toBe(status);
  });
});
