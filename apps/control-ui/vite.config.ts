import react from '@vitejs/plugin-react';
import { configDefaults, defineConfig } from 'vitest/config';

const tauriHost = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: tauriHost || '127.0.0.1',
    port: 1420,
    strictPort: true,
  },
  test: {
    exclude: [...configDefaults.exclude, 'scripts/**'],
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
