import { invoke, isTauri } from '@tauri-apps/api/core';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';

export function NativeMenuLocale() {
  const { i18n } = useTranslation();
  const locale = i18n.resolvedLanguage ?? i18n.language;

  useEffect(() => {
    if (!isTauri()) return;
    void invoke('set_tray_locale', { locale }).catch((error: unknown) => {
      console.error('Could not update native menu language', error);
    });
  }, [locale]);

  return null;
}
