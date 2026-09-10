import '@mantine/core/styles.css';

import { Badge, Button, createTheme, MantineProvider, Paper } from '@mantine/core';
import type { PropsWithChildren } from 'react';

export const mixrelayTheme = createTheme({
  colors: {
    signal: [
      '#f4f7fc',
      '#e8edf7',
      '#ced9ee',
      '#b3c4e4',
      '#9cb2db',
      '#8da8dc',
      '#7692ca',
      '#607bb3',
      '#4a6395',
      '#394b70',
    ],
    healthy: [
      '#eef8f4',
      '#dcefe8',
      '#badfd2',
      '#97ceba',
      '#7ebea7',
      '#70ad98',
      '#599481',
      '#457665',
      '#34594d',
      '#243d35',
    ],
  },
  components: {
    Badge: Badge.extend({ defaultProps: { radius: 999, tt: 'none', variant: 'light' } }),
    Button: Button.extend({ defaultProps: { radius: 0, variant: 'light' } }),
    Paper: Paper.extend({ defaultProps: { radius: 0 } }),
  },
  cursorType: 'default',
  defaultRadius: 0,
  fontFamily: 'Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  headings: {
    fontFamily: 'Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  },
  primaryColor: 'signal',
});

export function MixRelayProvider({ children }: PropsWithChildren) {
  return (
    <MantineProvider defaultColorScheme="dark" forceColorScheme="dark" theme={mixrelayTheme}>
      {children}
    </MantineProvider>
  );
}
