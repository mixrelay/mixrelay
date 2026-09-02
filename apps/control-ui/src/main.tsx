import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './app/App';
import { I18nProvider } from './core/i18n';
import { ToneNariProvider } from './core/theme/ToneNariProvider';
import './styles.css';

const root = document.getElementById('root');

if (!root) {
  throw new Error('ToneNari root element is missing');
}

createRoot(root).render(
  <StrictMode>
    <ToneNariProvider>
      <I18nProvider>
        <App />
      </I18nProvider>
    </ToneNariProvider>
  </StrictMode>,
);
