export type ColorScheme = 'light' | 'dark';
/** `system` follows the operating system's appearance. */
export type ThemePreference = ColorScheme | 'system';

export const defaultThemePreference: ThemePreference = 'system';
export const explicitColorSchemes: readonly ColorScheme[] = ['light', 'dark'];

/** Any stored or received value as a preference; anything unknown follows the system. */
export function themePreference(value: unknown): ThemePreference {
  return explicitColorSchemes.find((scheme) => scheme === value) ?? defaultThemePreference;
}
