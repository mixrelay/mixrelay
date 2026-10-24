import i18next from 'i18next';
import { type PropsWithChildren, useEffect } from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { resolveLocale } from './locale';
import { de } from './locales/de';
import { en } from './locales/en';
import { es } from './locales/es';
import { fr } from './locales/fr';
import { it } from './locales/it';
import { ja } from './locales/ja';
import { koKR } from './locales/ko-KR';
import { pseudo } from './locales/pseudo';
import { pt } from './locales/pt';
import { ru } from './locales/ru';
import { zhHans } from './locales/zh-Hans';
import { zhHant } from './locales/zh-Hant';

export type { SupportedLocale } from './locale';
export { formattingLocale, resolveLocale, supportedLocales } from './locale';

export const i18n = i18next.createInstance();

void i18n.use(initReactI18next).init({
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
  lng: resolveLocale([...navigator.languages, navigator.language]),
  resources: {
    en: { translation: en },
    'ko-KR': { translation: koKR },
    ja: { translation: ja },
    'zh-Hans': { translation: zhHans },
    'zh-Hant': { translation: zhHant },
    fr: { translation: fr },
    de: { translation: de },
    es: { translation: es },
    pt: { translation: pt },
    it: { translation: it },
    ru: { translation: ru },
    pseudo: { translation: pseudo },
  },
});

export function I18nProvider({ children }: PropsWithChildren) {
  useEffect(() => {
    const updateDocumentLanguage = () => {
      document.documentElement.lang = resolveLocale(i18n.resolvedLanguage ?? i18n.language);
    };
    updateDocumentLanguage();
    i18n.on('languageChanged', updateDocumentLanguage);
    return () => {
      i18n.off('languageChanged', updateDocumentLanguage);
    };
  }, []);
  return <I18nextProvider i18n={i18n}>{children}</I18nextProvider>;
}
