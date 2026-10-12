import path from 'node:path';
import { fileURLToPath } from 'node:url';
import react from '@vitejs/plugin-react';
import { type HtmlTagDescriptor, type Plugin, runnerImport } from 'vite';
import { configDefaults, defineConfig } from 'vitest/config';

/** Puts the token theme CSS and the saved-preference bootstrap at the start of <head>.
 * The tags come from src/core/theme/themeDocument.ts, loaded through Vite's module
 * runner so this config never imports application source directly. Only the app's
 * index.html is changed; Storybook's iframe document is left to its own Provider. */
function mixrelayThemeDocument(): Plugin {
  const module = fileURLToPath(new URL('./src/core/theme/themeDocument.ts', import.meta.url));
  const appDocument = fileURLToPath(new URL('./index.html', import.meta.url));
  return {
    name: 'mixrelay-theme-document',
    async transformIndexHtml(_html, context) {
      // Only the application document: Storybook's iframe keeps its controlled theme.
      if (path.resolve(context.filename) !== appDocument) return undefined;
      const { module: theme } = await runnerImport<{
        themeDocumentTags: () => HtmlTagDescriptor[];
      }>(module, { configFile: false, logLevel: 'silent' });
      return theme.themeDocumentTags();
    },
  };
}

const tauriHost = process.env.TAURI_DEV_HOST;
// Story accessibility rendering is heavy; it runs as its own step so it never competes
// with timing-sensitive behaviour tests (pnpm test:a11y).
const a11yRun = process.env.MIXRELAY_UI_A11Y === '1';
const a11yTests = 'src/**/*.a11y.test.tsx';

export default defineConfig({
  plugins: [react(), mixrelayThemeDocument()],
  clearScreen: false,
  server: {
    host: tauriHost || '127.0.0.1',
    port: 1420,
    strictPort: true,
  },
  test: {
    exclude: [...configDefaults.exclude, 'scripts/**', ...(a11yRun ? [] : [a11yTests])],
    ...(a11yRun ? { include: [a11yTests] } : {}),
    maxWorkers: Number(process.env.MIXRELAY_UI_TEST_WORKERS ?? 4),
    coverage: {
      reportsDirectory: process.env.MIXRELAY_UI_COVERAGE_DIR ?? './coverage',
      exclude: [
        // Storybook-only documentation is verified in its built browser surface.
        'src/core/design-system/**',
        'src/**/*.stories.{ts,tsx}',
        'src/**/*.test.{ts,tsx}',
        'src/main.tsx',
        'src/test-setup.ts',
      ],
      include: ['src/**/*.{ts,tsx}'],
      provider: 'v8',
      reporter: ['text', 'json'],
      thresholds: {
        branches: 100,
        functions: 100,
        lines: 100,
        statements: 100,
      },
    },
    environment: 'jsdom',
    setupFiles: './src/test-setup.ts',
  },
});
