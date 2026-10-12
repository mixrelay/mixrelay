import { render, waitFor } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import { MixRelayProvider } from './MixRelayProvider';
import { NativeWindowTheme } from './NativeWindowTheme';

const native = vi.hoisted(() => ({ isTauri: vi.fn(), setTheme: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ isTauri: native.isTauri }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ setTheme: native.setTheme }),
}));

beforeEach(() => {
  native.isTauri.mockReset().mockReturnValue(true);
  native.setTheme.mockReset().mockResolvedValue(undefined);
});

it('sets the native window to the chosen appearance and releases it for system', () => {
  const view = render(
    <MixRelayProvider theme="dark">
      <NativeWindowTheme />
    </MixRelayProvider>,
  );
  expect(native.setTheme).toHaveBeenLastCalledWith('dark');
  view.rerender(
    <MixRelayProvider theme="system">
      <NativeWindowTheme />
    </MixRelayProvider>,
  );
  expect(native.setTheme).toHaveBeenLastCalledWith(null);
});

it('does nothing in the browser and reports native failures without throwing', async () => {
  native.isTauri.mockReturnValue(false);
  const view = render(
    <MixRelayProvider theme="light">
      <NativeWindowTheme />
    </MixRelayProvider>,
  );
  expect(native.setTheme).not.toHaveBeenCalled();

  native.isTauri.mockReturnValue(true);
  native.setTheme.mockRejectedValue(new Error('denied'));
  const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
  view.rerender(
    <MixRelayProvider theme="dark">
      <NativeWindowTheme />
    </MixRelayProvider>,
  );
  await waitFor(() =>
    expect(error).toHaveBeenCalledWith(
      'Could not update the native window appearance',
      expect.any(Error),
    ),
  );
});
