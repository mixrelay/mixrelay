import type { ThemePreference } from './tokens';

export const themeStorageKey = 'mixrelay.theme';

export function themePreference(value: unknown): ThemePreference {
  return value === 'light' || value === 'dark' ? value : 'system';
}

export function readThemePreference(): ThemePreference {
  try {
    return themePreference(localStorage.getItem(themeStorageKey));
  } catch {
    return 'system';
  }
}

export function saveThemePreference(value: ThemePreference) {
  try {
    localStorage.setItem(themeStorageKey, value);
  } catch {
    // A blocked store must not prevent changing this session's appearance.
  }
}
