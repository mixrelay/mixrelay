import i18next from 'i18next';
import type { PropsWithChildren } from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';

import { en } from './locales/en';
import { koKR } from './locales/ko-KR';
import { pseudo } from './locales/pseudo';

export const i18n = i18next.createInstance();

export function resolveLocale(language: string) {
  return language.startsWith('ko') ? 'ko-KR' : 'en';
}

void i18n.use(initReactI18next).init({
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
  lng: resolveLocale(navigator.language),
  resources: {
    en: { translation: en },
    'ko-KR': { translation: koKR },
    pseudo: { translation: pseudo },
  },
});

export function I18nProvider({ children }: PropsWithChildren) {
  return <I18nextProvider i18n={i18n}>{children}</I18nextProvider>;
}
