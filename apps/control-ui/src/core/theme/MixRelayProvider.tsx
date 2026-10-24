import './Globals.css';
import {
  createContext,
  type PropsWithChildren,
  useContext,
  useEffect,
  useLayoutEffect,
  useState,
} from 'react';
import { I18nProvider } from 'react-aria-components';
import { useTranslation } from 'react-i18next';
import { themeCss } from './theme';
import {
  readThemePreference,
  saveThemePreference,
  themePreference,
  themeStorageKey,
} from './themePreference';
import type { ThemePreference } from './tokens';

const ThemeContext = createContext<{
  preference: ThemePreference;
  setPreference: (value: ThemePreference) => void;
}>({ preference: 'system', setPreference: () => undefined });

export const useTheme = () => useContext(ThemeContext);

/** App and Storybook share one visual owner; React Aria supplies behavior only. */
export function MixRelayProvider({
  children,
  theme,
  onThemeChange,
}: PropsWithChildren<{
  theme?: ThemePreference;
  onThemeChange?: (value: ThemePreference) => void;
}>) {
  const [savedPreference, setSavedPreference] = useState(readThemePreference);
  const preference = theme ?? savedPreference;
  const setPreference = (value: ThemePreference) => {
    if (theme !== undefined) {
      onThemeChange?.(value);
    } else {
      setSavedPreference(value);
      saveThemePreference(value);
    }
  };
  useLayoutEffect(() => {
    const root = document.documentElement;
    const previous = root.getAttribute('data-theme');
    root.setAttribute('data-theme', preference);
    return () => {
      if (previous === null) root.removeAttribute('data-theme');
      else root.setAttribute('data-theme', previous);
    };
  }, [preference]);
  useEffect(() => {
    const synchronize = (event: StorageEvent) => {
      if (event.key === themeStorageKey || event.key === null)
        setSavedPreference(themePreference(event.newValue));
    };
    window.addEventListener('storage', synchronize);
    return () => window.removeEventListener('storage', synchronize);
  }, []);
  const { i18n } = useTranslation();
  const locale = i18n.resolvedLanguage === 'pseudo' ? 'en' : (i18n.resolvedLanguage ?? 'en');
  return (
    <I18nProvider locale={locale}>
      <ThemeContext.Provider value={{ preference, setPreference }}>
        <style data-mixrelay-theme>{themeCss}</style>
        {children}
      </ThemeContext.Provider>
    </I18nProvider>
  );
}
