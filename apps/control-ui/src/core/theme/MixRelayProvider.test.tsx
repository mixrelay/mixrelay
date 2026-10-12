import { render } from '@testing-library/react';
import { expect, it } from 'vitest';

import { MixRelayProvider } from './MixRelayProvider';
import { mixrelayVariables, themeRoles } from './tokens';

it('publishes shared tokens to app and Storybook consumers', () => {
  const { container } = render(
    <MixRelayProvider>
      <span>MixRelay</span>
    </MixRelayProvider>,
  );
  const css = Array.from(container.querySelectorAll('style[data-mixrelay-theme]'))
    .map((style) => style.textContent)
    .join('\n');
  expect(css).toContain(`--bg:${mixrelayVariables['--bg']}`);
  expect(css).toContain(`--status-accent:${themeRoles.light['--status-accent']}`);
  expect(css).toContain(`--status-accent:${themeRoles.dark['--status-accent']}`);
  expect(css).toContain('--positive:var(--status-positive)');
  expect(css).toContain('--transport-background-active:var(--status-danger-surface)');
  expect(css).toContain(`--meter-normal:${mixrelayVariables['--meter-normal']}`);
  expect(css).toContain('--transport-size:var(--control-height)');
  expect(css).toContain('--form-actions-gap:var(--space-2)');
  expect(css).toContain('--form-actions-inset:var(--space-3)');
});
