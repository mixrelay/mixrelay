import type { Preview } from '@storybook/react-vite';
import { I18nextProvider } from 'react-i18next';

import { i18n } from '../src/core/i18n';
import { ToneNariProvider } from '../src/core/theme/ToneNariProvider';
import '../src/styles.css';

const preview: Preview = {
  decorators: [
    (Story, context) => {
      void i18n.changeLanguage(context.globals.locale as string);
      return (
        <ToneNariProvider>
          <I18nextProvider i18n={i18n}>
            <div className="storybook-frame">
              <Story />
            </div>
          </I18nextProvider>
        </ToneNariProvider>
      );
    },
  ],
  globalTypes: {
    locale: {
      description: 'UI locale',
      toolbar: {
        icon: 'globe',
        items: [
          { value: 'ko-KR', title: '한국어' },
          { value: 'en', title: 'English' },
          { value: 'pseudo', title: 'Pseudo' },
        ],
        dynamicTitle: true,
      },
    },
  },
  initialGlobals: {
    locale: 'ko-KR',
  },
  parameters: {
    a11y: {
      test: 'error',
    },
    controls: {
      expanded: true,
    },
    viewport: {
      options: {
        mobile: { name: 'Mobile narrow', styles: { width: '390px', height: '844px' } },
        tablet: { name: 'Tablet', styles: { width: '768px', height: '1024px' } },
        desktop: { name: 'Desktop', styles: { width: '1280px', height: '800px' } },
      },
    },
  },
};

export default preview;
