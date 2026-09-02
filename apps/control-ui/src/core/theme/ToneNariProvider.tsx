import '@mantine/core/styles.css';

import { Badge, Button, createTheme, MantineProvider, Paper } from '@mantine/core';
import type { PropsWithChildren } from 'react';

export const tonenariTheme = createTheme({
  colors: {
    signal: [
      '#effff9',
      '#d8fff0',
      '#acfde0',
      '#7cf9cd',
      '#58f3bd',
      '#43e9b0',
      '#32d09b',
      '#20ad80',
      '#0d8964',
      '#006f50',
    ],
  },
  components: {
    Badge: Badge.extend({ defaultProps: { radius: 999, tt: 'none', variant: 'light' } }),
    Button: Button.extend({ defaultProps: { radius: 0, variant: 'light' } }),
    Paper: Paper.extend({ defaultProps: { radius: 0 } }),
  },
  cursorType: 'pointer',
  defaultRadius: 0,
  fontFamily: 'Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  headings: {
    fontFamily: 'Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  },
  primaryColor: 'signal',
});

export function ToneNariProvider({ children }: PropsWithChildren) {
  return (
    <MantineProvider defaultColorScheme="dark" forceColorScheme="dark" theme={tonenariTheme}>
      {children}
    </MantineProvider>
  );
}
