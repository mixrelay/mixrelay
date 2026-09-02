import { fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { describe, expect, it, vi } from 'vitest';

import { i18n } from '../../core/i18n';
import { ToneNariProvider } from '../../core/theme/ToneNariProvider';
import { DriverStatusCard } from './DriverStatusCard';

describe('DriverStatusCard', () => {
  it('maps a stable machine state to localized UI and action', async () => {
    await i18n.changeLanguage('en');
    const onAction = vi.fn();

    render(
      <ToneNariProvider>
        <I18nextProvider i18n={i18n}>
          <DriverStatusCard onAction={onAction} state="restart_required" />
        </I18nextProvider>
      </ToneNariProvider>,
    );

    expect(screen.getByText('Restart required')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Restart audio services' }));
    expect(onAction).toHaveBeenCalledOnce();
  });
});
