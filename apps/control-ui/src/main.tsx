import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app/App';
import { I18nProvider } from './core/i18n';
import { MixRelayProvider } from './core/theme/MixRelayProvider';
import './styles.css';

const root = document.getElementById('root');

if (!root) {
  throw new Error('MixRelay root element is missing');
}

createRoot(root).render(
  <StrictMode>
    <MixRelayProvider>
      <I18nProvider>
        <App />
      </I18nProvider>
    </MixRelayProvider>
  </StrictMode>,
);
