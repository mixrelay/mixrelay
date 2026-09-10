import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { ToneNariProvider } from '../theme/ToneNariProvider';
import { AudioNodeCard, type AudioNodeCardProps } from './AudioNodeCard';

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

it('places native plug-in content under a compact unmetered face', () => {
  const view = render(
    <ToneNariProvider>
      <AudioNodeCard
        category="Processor"
        icon="AU"
        title="Filter"
        inputCount={2}
        outputCount={2}
        faceMode="compact"
        faceHeight={36}
        onSelect={vi.fn()}
        content={<div>Native editor</div>}
      />
    </ToneNariProvider>,
  );
  const root = view.getByText('Native editor').closest('[data-face-mode]');
  expect(root).toHaveAttribute('data-face-mode', 'compact');
  expect(root).toHaveStyle('--audio-node-face-height: 36px');
  expect(root).not.toHaveAttribute('data-metered');
  expect(view.queryByText('IN 2')).not.toBeInTheDocument();
  expect(view.queryByText('OUT 2')).not.toBeInTheDocument();
});

it.each(['closed', 'opening', 'open', 'failed', 'unavailable', undefined] as const)(
  'prefers the plug-in editor with parameter access when its state is %s',
  (state) => {
    const onToggle = vi.fn();
    const editor: AudioNodeCardProps['processorEditor'] = state
      ? { actionLabel: 'Retry editor', state }
      : undefined;
    const view = render(
      <ToneNariProvider>
        <AudioNodeCard
          category="Processor"
          icon="FX"
          inputCount={2}
          outputCount={2}
          title="Filter"
          onSelect={vi.fn()}
          processorEditor={editor}
          onProcessorEditorRetry={onToggle}
          onParameterChange={vi.fn()}
          parameterControls={[
            {
              id: 'cutoff',
              label: 'Cutoff',
              minimum: 0,
              maximum: 1,
              step: 0.01,
              suffix: '',
              value: 0.5,
            },
          ]}
        />
      </ToneNariProvider>,
    );
    const needsParameters = !state || state === 'failed' || state === 'unavailable';
    if (needsParameters) {
      fireEvent.click(view.getByRole('button', { name: 'Options' }));
      expect(view.getByRole('slider', { name: 'Cutoff' })).toBeEnabled();
    } else {
      expect(view.queryByRole('button', { name: 'Options' })).not.toBeInTheDocument();
      expect(view.queryByRole('slider')).not.toBeInTheDocument();
    }
    if (state === 'failed') {
      const button = view.getByRole('button', { name: 'Retry editor' });
      expect(button.textContent).toBe('');
      expect(view.queryByText('Plug-in editor')).not.toBeInTheDocument();
      expect(view.queryByText(state)).not.toBeInTheDocument();
      expect(button.closest('.nodrag.nopan.nowheel')).not.toBeNull();
      fireEvent.click(button);
      expect(onToggle).toHaveBeenCalledOnce();
    } else {
      expect(view.queryByRole('button', { name: 'Retry editor' })).not.toBeInTheDocument();
    }
  },
);

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

it('clears offline meters while keeping selection and options editable, then restores live levels', () => {
  const onSelect = vi.fn();
  const props = {
    category: 'Input',
    icon: 'MIC',
    title: 'Microphone',
    inputCount: 0,
    outputCount: 2,
    meterChannels: 2,
    meterLevels: [60, 40],
    onSelect,
    gainControl: { label: 'Gain', valueDb: 0 },
    onGainChange: vi.fn(),
  };
  const view = render(
    <ToneNariProvider>
      <AudioNodeCard {...props} offline status="Offline" />
    </ToneNariProvider>,
  );
  const button = view.getByRole('button', { name: 'Microphone, Input, Offline' });
  expect(button).toBeEnabled();
  expect(button.closest('[data-offline]')).not.toBeNull();
  expect(view.container.querySelectorAll('[title$=": 0%"]')).toHaveLength(2);
  fireEvent.click(button);
  expect(onSelect).toHaveBeenCalledOnce();
  fireEvent.click(view.getByRole('button', { name: 'Options' }));
  expect(view.getByRole('slider', { name: 'Gain' })).toBeEnabled();
  view.rerender(
    <ToneNariProvider>
      <AudioNodeCard {...props} />
    </ToneNariProvider>,
  );
  expect(view.container.querySelector('[data-offline]')).toBeNull();
  expect(view.container.querySelector('[title$=": 60%"]')).not.toBeNull();
  expect(view.container.querySelector('[title$=": 40%"]')).not.toBeNull();
});
