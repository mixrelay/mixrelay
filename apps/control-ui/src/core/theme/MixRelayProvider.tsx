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
import { browserThemePreferenceStorage } from '../adapters/browser/themePreferenceStorage';
import type { ThemePreference } from '../domain/appearance';
import type { ThemePreferenceStore } from '../ports/ThemePreferenceStore';
import { themeCss } from './theme';

const ThemeContext = createContext<{
  preference: ThemePreference;
  setPreference: (value: ThemePreference) => void;
}>({ preference: 'system', setPreference: () => undefined });

export const useTheme = () => useContext(ThemeContext);

const browserStore = browserThemePreferenceStorage();

/** App and Storybook share one visual owner; React Aria supplies behavior only. */
export function MixRelayProvider({
  children,
  theme,
  onThemeChange,
  store = browserStore,
}: PropsWithChildren<{
  theme?: ThemePreference;
  onThemeChange?: (value: ThemePreference) => void;
  /** Where an uncontrolled preference is kept; the page's local storage by default. */
  store?: ThemePreferenceStore;
}>) {
  const [savedPreference, setSavedPreference] = useState(() => store.read());
  const preference = theme ?? savedPreference;
  const setPreference = (value: ThemePreference) => {
    if (theme !== undefined) {
      onThemeChange?.(value);
    } else {
      setSavedPreference(value);
      store.write(value);
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
  useEffect(() => store.subscribe(setSavedPreference), [store]);
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
