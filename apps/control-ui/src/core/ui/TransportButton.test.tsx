import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { ToneNariProvider } from '../theme/ToneNariProvider';
import { TransportButton } from './TransportButton';

it.each(['Record', '녹음', '[Record]'])(
  'provides accessible icon-only transport and focus tooltip: %s',
  async (label) => {
    const click = vi.fn();
    render(
      <ToneNariProvider>
        <TransportButton icon="record" label={label} disabled={false} onClick={click} />
      </ToneNariProvider>,
    );
    const button = screen.getByRole('button', { name: label });
    expect(button.textContent).toBe('');
    expect(button.querySelector('circle')).not.toBeNull();
    fireEvent.focus(button);
    expect(await screen.findByRole('tooltip')).toHaveTextContent(label);
    fireEvent.click(button);
    expect(click).toHaveBeenCalledTimes(1);
  },
);

it('retains the active recording indicator when the record action is disabled', () => {
  render(
    <ToneNariProvider>
      <TransportButton icon="record" label="Record" active disabled onClick={vi.fn()} />
    </ToneNariProvider>,
  );
  const button = screen.getByRole('button', { name: 'Record' });
  expect(button).toHaveAttribute('data-active', 'true');
  expect(button).toHaveAttribute('aria-pressed', 'true');
  expect(button).toBeDisabled();
});

it.each(['record', 'pause', 'stop', 'resume'] as const)(
  'uses the same button surface and icon size for %s',
  (icon) => {
    render(
      <ToneNariProvider>
        <TransportButton icon={icon} label={icon} disabled={false} onClick={vi.fn()} />
      </ToneNariProvider>,
    );
    const button = screen.getByRole('button', { name: icon });
    expect(button).toHaveAttribute('data-transport', icon);
    expect(button).toHaveAttribute('data-variant', 'default');
    expect(button.querySelector('svg')).toHaveAttribute('width', '16');
    expect(button.querySelector('svg')).toHaveAttribute('height', '16');
  },
);
