import type { StorybookConfig } from '@storybook/react-vite';

const STORYBOOK_CHUNK_WARNING_LIMIT_KIB = 1_300;

const config: StorybookConfig = {
  stories: ['../src/**/*.stories.@(ts|tsx)'],
  addons: ['@storybook/addon-a11y'],
  framework: '@storybook/react-vite',
  viteFinal: (viteConfig) => ({
    ...viteConfig,
    build: {
      ...viteConfig.build,
      chunkSizeWarningLimit: STORYBOOK_CHUNK_WARNING_LIMIT_KIB,
    },
  }),
};

export default config;
