import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { ToneNariProvider } from '../theme/ToneNariProvider';
import { AudioNodeCard } from './AudioNodeCard';

function card(iconUrl?: string) {
  return (
    <ToneNariProvider>
      <AudioNodeCard
        category="Application"
        icon="APP"
        iconUrl={iconUrl}
        inputCount={0}
        outputCount={2}
        title="Player"
        onSelect={vi.fn()}
      />
    </ToneNariProvider>
  );
}

it('renders feature-owned controls without changing the port face boundary', () => {
  const view = render(
    <ToneNariProvider>
      <AudioNodeCard
        category="Output"
        icon="REC"
        title="Recorder"
        inputCount={2}
        outputCount={0}
        faceHeight={160}
        onSelect={vi.fn()}
        content={<button type="button">Record</button>}
      />
    </ToneNariProvider>,
  );
  const control = view.getByRole('button', { name: 'Record' });
  expect(control.closest('.nodrag.nopan.nowheel')).not.toBeNull();
  const cardRoot = control.closest('[data-metered], [style*="--audio-node-face-height"]');
  expect(cardRoot?.getAttribute('style')).toContain('--audio-node-face-height: 160px');
});

describe('application node icon fallback', () => {
  it('uses the application glyph when a supplied icon cannot decode, and retries changed content', () => {
    const view = render(card('data:image/png;base64,broken'));
    const image = view.container.querySelector('img');
    expect(image).not.toBeNull();
    if (!image) throw new Error('Expected native application icon');
    fireEvent.error(image);
    expect(view.container.querySelector('img')).toBeNull();
    expect(view.container.querySelector('svg')).not.toBeNull();
    view.rerender(card('data:image/png;base64,replacement'));
    expect(view.container.querySelector('img')).toHaveAttribute(
      'src',
      'data:image/png;base64,replacement',
    );
  });

  it('keeps fallback glyphs for applications without an icon', () => {
    const view = render(card());
    expect(view.container.querySelector('img')).toBeNull();
    expect(view.container.querySelector('svg')).not.toBeNull();
  });
});
