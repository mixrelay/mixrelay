import { render, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { NativeMenuLocale } from './NativeMenuLocale';

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(),
  i18n: { resolvedLanguage: 'ko-KR', language: 'ko-KR' },
}));

vi.mock('@tauri-apps/api/core', () => native);
vi.mock('react-i18next', () => ({ useTranslation: () => ({ i18n: native.i18n }) }));

describe('NativeMenuLocale', () => {
  beforeEach(() => {
    native.invoke.mockReset().mockResolvedValue(undefined);
    native.isTauri.mockReturnValue(true);
    native.i18n.resolvedLanguage = 'ko-KR';
  });

  it('sends the resolved UI language and updates when it changes', () => {
    const view = render(<NativeMenuLocale />);
    expect(native.invoke).toHaveBeenLastCalledWith('set_native_menu_locale', {
      locale: 'ko-KR',
    });
    native.i18n.resolvedLanguage = 'en';
    view.rerender(<NativeMenuLocale />);
    expect(native.invoke).toHaveBeenLastCalledWith('set_native_menu_locale', { locale: 'en' });
  });

  it('does not invoke native commands in the browser', () => {
    native.isTauri.mockReturnValue(false);
    render(<NativeMenuLocale />);
    expect(native.invoke).not.toHaveBeenCalled();
  });

  it('reports native failures without an unhandled rejection', async () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    native.invoke.mockRejectedValue('native menu unavailable');
    render(<NativeMenuLocale />);
    await waitFor(() =>
      expect(log).toHaveBeenCalledWith(
        'Could not update native menu language',
        'native menu unavailable',
      ),
    );
    log.mockRestore();
  });
});
