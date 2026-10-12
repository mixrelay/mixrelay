import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app/App';
import { I18nProvider } from './core/i18n';
import { NativeMenuLocale } from './core/i18n/NativeMenuLocale';
import { MixRelayProvider } from './core/theme/MixRelayProvider';

const root = document.getElementById('root');

if (!root) {
  throw new Error('MixRelay root element is missing');
}

createRoot(root).render(
  <StrictMode>
    <MixRelayProvider>
      <I18nProvider>
        <NativeMenuLocale />
        <App />
      </I18nProvider>
    </MixRelayProvider>
  </StrictMode>,
);
