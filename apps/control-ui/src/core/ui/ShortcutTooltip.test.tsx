import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';

import { ToneNariProvider } from '../theme/ToneNariProvider';
import { formatPlatformShortcut } from './platformShortcut';
import { ShortcutTooltip } from './ShortcutTooltip';

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it.each([
  ['MacIntel', '⌘N'],
  ['Win32', 'Ctrl+N'],
  ['Linux x86_64', 'Ctrl+N'],
  ['Linux aarch64', 'Ctrl+N'],
])('renders the new graph tooltip for %s after the hover delay', async (platform, label) => {
  vi.useFakeTimers();
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue(platform);
  render(
    <ToneNariProvider>
      <ShortcutTooltip shortcut="Mod+N">
        <button type="button">New graph</button>
      </ShortcutTooltip>
    </ToneNariProvider>,
  );
  fireEvent.mouseEnter(screen.getByRole('button', { name: 'New graph' }));
  expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  await act(async () => vi.advanceTimersByTimeAsync(350));
  // Mantine mounts its portal transition on the following animation frame.
  await act(async () => vi.advanceTimersByTimeAsync(200));
  expect(screen.getByRole('tooltip')).toHaveTextContent(label);
});

it('does not rewrite explicit non-Mod keys or unknown shortcuts', () => {
  expect(formatPlatformShortcut('Ctrl+Shift+Z', 'MacIntel')).toBe('Ctrl+Shift+Z');
  expect(formatPlatformShortcut('Space', 'Win32')).toBe('Space');
  expect(formatPlatformShortcut('Mod+Z', '')).toBe('Ctrl+Z');
});
