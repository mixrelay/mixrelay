import { act, fireEvent, render, waitFor } from '@testing-library/react';
import { assert, describe, expect, it, vi } from 'vitest';

import { AudioMeterProvider } from '../audio-meter/AudioMeterProvider';
import { AudioMeterStore } from '../audio-meter/AudioMeterStore';
import { MixRelayProvider } from '../theme/MixRelayProvider';
import { AudioNodeCard, type AudioNodeCardProps } from './AudioNodeCard';

function card(iconUrl?: string) {
  return (
    <MixRelayProvider>
      <AudioNodeCard
        category="Application"
        icon="APP"
        iconUrl={iconUrl}
        inputCount={0}
        outputCount={2}
        title="Player"
        onSelect={vi.fn()}
      />
    </MixRelayProvider>
  );
}

function installResizeObserverHarness() {
  const original = globalThis.ResizeObserver;
  const callbacks = new Map<Element, ResizeObserverCallback>();
  globalThis.ResizeObserver = class implements ResizeObserver {
    constructor(private readonly callback: ResizeObserverCallback) {}
    observe(target: Element) {
      callbacks.set(target, this.callback);
    }
    unobserve(target: Element) {
      callbacks.delete(target);
    }
    disconnect() {}
  };
  return {
    restore: () => {
      globalThis.ResizeObserver = original;
    },
    emit: (target: Element, height: number, width = 300) => {
      const callback = callbacks.get(target);
      assert(callback);
      act(() =>
        callback(
          [
            {
              target,
              borderBoxSize: [{ blockSize: height, inlineSize: width }],
              contentRect: { x: 0, y: 0, top: 0, left: 0, right: width, bottom: height },
            } as unknown as ResizeObserverEntry,
          ],
          {} as ResizeObserver,
        ),
      );
    },
  };
}

it('reports one DOM-coherent controls/disclosure pair across observer order and reflow', async () => {
  const { emit, restore } = installResizeObserverHarness();
  try {
    const onContentHeightChange = vi.fn();
    const view = render(
      <MixRelayProvider>
        <AudioNodeCard
          category="Input"
          icon="MIC"
          title="Input"
          inputCount={0}
          outputCount={2}
          onSelect={vi.fn()}
          gainControl={{ label: 'Gain', valueDb: 0 }}
          onGainChange={vi.fn()}
          onContentHeightChange={onContentHeightChange}
        />
      </MixRelayProvider>,
    );
    const options = view.getByRole('button', { name: 'Options' });
    const controls = options.parentElement?.parentElement as HTMLElement;
    assert(controls);
    controls.style.boxSizing = 'border-box';
    controls.style.height = '25px';
    emit(controls, 25);
    await waitFor(() => expect(onContentHeightChange).toHaveBeenLastCalledWith(25, 0));

    fireEvent.click(options);
    const panel = options.nextElementSibling as HTMLElement;
    assert(panel);
    panel.style.boxSizing = 'border-box';
    panel.style.height = '37.5px';
    controls.style.height = '62.5px';
    emit(panel, 37.5);
    emit(controls, 62.5);
    await waitFor(() => expect(onContentHeightChange).toHaveBeenLastCalledWith(62.5, 37.5));

    onContentHeightChange.mockClear();
    controls.style.height = '85px';
    panel.style.height = '60px';
    emit(controls, 85);
    await waitFor(() => expect(onContentHeightChange).toHaveBeenLastCalledWith(85, 60));
    expect(
      onContentHeightChange.mock.calls.every(([total, disclosure]) => total - disclosure === 25),
    ).toBe(true);
    emit(panel, 60);

    onContentHeightChange.mockClear();
    controls.style.height = '55px';
    panel.style.height = '30px';
    emit(panel, 30);
    await waitFor(() => expect(onContentHeightChange).toHaveBeenLastCalledWith(55, 30));
    expect(
      onContentHeightChange.mock.calls.every(([total, disclosure]) => total - disclosure === 25),
    ).toBe(true);
    emit(controls, 55);

    controls.style.height = '69px';
    panel.style.height = '44px';
    emit(panel, 30, 240);
    await waitFor(() => expect(onContentHeightChange).toHaveBeenLastCalledWith(69, 44));
  } finally {
    restore();
  }
});

it('renders every generic plug-in parameter without an internal scroll viewport', () => {
  const view = render(
    <MixRelayProvider>
      <AudioNodeCard
        category="Processor"
        icon="AU"
        title="Reverb"
        inputCount={2}
        outputCount={2}
        faceMode="compact"
        onSelect={vi.fn()}
        onParameterChange={vi.fn()}
        parameterControls={Array.from({ length: 24 }, (_, index) => ({
          id: `parameter-${index}`,
          label: `Parameter ${index + 1}`,
          minimum: 0,
          maximum: 1,
          step: 0.01,
          suffix: '',
          value: 0.5,
        }))}
      />
    </MixRelayProvider>,
  );
  expect(view.queryByRole('button', { name: 'Options' })).not.toBeInTheDocument();
  expect(view.getAllByRole('slider')).toHaveLength(24);
  expect(view.getByRole('slider', { name: 'Parameter 24' })).toBeVisible();
  expect(view.container.querySelector('[data-radix-scroll-area-viewport]')).toBeNull();
  expect(view.container.querySelector('.mantine-ScrollArea-root')).toBeNull();
});

it('places native plug-in content under a compact unmetered face', () => {
  const view = render(
    <MixRelayProvider>
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
    </MixRelayProvider>,
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
      ? { actionLabel: state === 'closed' ? 'Open editor' : 'Retry editor', state }
      : undefined;
    const view = render(
      <MixRelayProvider>
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
      </MixRelayProvider>,
    );
    const needsParameters = !state || state === 'failed' || state === 'unavailable';
    if (needsParameters) {
      expect(view.queryByRole('button', { name: 'Options' })).not.toBeInTheDocument();
      expect(view.getByRole('slider', { name: 'Cutoff' })).toBeEnabled();
    } else {
      expect(view.queryByRole('button', { name: 'Options' })).not.toBeInTheDocument();
      expect(view.queryByRole('slider')).not.toBeInTheDocument();
    }
    if (state === 'failed' || state === 'closed') {
      const button = view.getByRole('button', {
        name: state === 'closed' ? 'Open editor' : 'Retry editor',
      });
      expect(button.textContent).toBe('');
      expect(view.queryByText('Plug-in editor')).not.toBeInTheDocument();
      expect(view.queryByText(state)).not.toBeInTheDocument();
      expect(button.closest('.nodrag.nopan.nowheel')).not.toBeNull();
      fireEvent.click(button);
      expect(onToggle).toHaveBeenCalledOnce();
    } else {
      expect(view.queryByRole('button', { name: 'Retry editor' })).not.toBeInTheDocument();
      expect(view.queryByRole('button', { name: 'Open editor' })).not.toBeInTheDocument();
    }
  },
);

it('renders feature-owned controls without changing the port face boundary', () => {
  const view = render(
    <MixRelayProvider>
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
    </MixRelayProvider>,
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
  const store = new AudioMeterStore();
  store.update({
    nodes: [{ nodeId: 'microphone', portId: 'audio', peaks: [1, 0] }],
    protocolVersion: 1,
    revision: 1,
  });
  const props = {
    category: 'Input',
    icon: 'MIC',
    title: 'Microphone',
    inputCount: 0,
    outputCount: 2,
    meterBanks: [{ channels: 2, nodeId: 'microphone', portId: 'audio' }],
    onSelect,
    gainControl: { label: 'Gain', valueDb: 0 },
    onGainChange: vi.fn(),
  };
  const view = render(
    <MixRelayProvider>
      <AudioMeterProvider store={store}>
        <AudioNodeCard {...props} offline status="Offline" />
      </AudioMeterProvider>
    </MixRelayProvider>,
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
    <MixRelayProvider>
      <AudioMeterProvider store={store}>
        <AudioNodeCard {...props} />
      </AudioMeterProvider>
    </MixRelayProvider>,
  );
  expect(view.container.querySelector('[data-offline]')).toBeNull();
  expect(view.container.querySelector('[title$=": 100%"]')).not.toBeNull();
  expect(view.container.querySelector('[title$=": 0%"]')).not.toBeNull();
});

it.each([
  { channels: 8, density: undefined },
  { channels: 9, density: 'compact' },
  { channels: 32, density: 'compact' },
  { channels: 64, density: 'dense' },
])(
  'renders an exact $channels-channel meter bank without truncating its channels',
  ({ channels, density }) => {
    const view = render(
      <MixRelayProvider>
        <AudioNodeCard
          category="Processor"
          icon="FX"
          inputCount={channels}
          outputCount={channels}
          meterBanks={[{ channels, nodeId: 'processor', portId: 'audio' }]}
          title={`${channels} channel processor`}
          onSelect={vi.fn()}
        />
      </MixRelayProvider>,
    );
    const meterContainer = view.container.querySelector(`[data-channels="${channels}"]`);
    expect(meterContainer).not.toBeNull();
    if (channels <= 8) {
      expect(meterContainer?.querySelectorAll('[data-orientation="horizontal"]')).toHaveLength(
        channels,
      );
    } else {
      expect(meterContainer).toHaveAttribute('data-density', density);
      expect(meterContainer?.querySelectorAll('[data-orientation="vertical"]')).toHaveLength(
        channels,
      );
    }
  },
);

it('binds provider banks to their authored output IDs and keeps peak levels separate', () => {
  const store = new AudioMeterStore();
  store.update({
    nodes: [
      { nodeId: 'provider', portId: 'audio', peaks: [1, 1] },
      { nodeId: 'provider', portId: 'bus:1', peaks: [0] },
    ],
    protocolVersion: 1,
    revision: 2,
  });
  const view = render(
    <MixRelayProvider>
      <AudioMeterProvider store={store}>
        <AudioNodeCard
          category="Processor"
          icon="VST3"
          inputCount={2}
          outputCount={3}
          faceMode="compact"
          meterBanks={[
            { channels: 2, nodeId: 'provider', portId: 'audio', label: 'Main · Output' },
            { channels: 1, nodeId: 'provider', portId: 'bus:1', label: 'Sidechain · Detector' },
          ]}
          title="Dynamics"
          onSelect={vi.fn()}
        />
      </AudioMeterProvider>
    </MixRelayProvider>,
  );
  expect(view.getByText('Main · Output')).toBeVisible();
  expect(view.getByText('Sidechain · Detector')).toBeVisible();
  expect(
    view.container.querySelector('[title="Dynamics Main · Output channel 1: 100%"]'),
  ).toBeTruthy();
  expect(
    view.container.querySelector('[title="Dynamics Sidechain · Detector channel 1: 0%"]'),
  ).toBeTruthy();
});

it('keeps both directions of a duplex endpoint on their own visible meter banks', () => {
  const store = new AudioMeterStore();
  store.update({
    nodes: [
      { nodeId: 'duplex', portId: 'from_os', peaks: [1, 0] },
      { nodeId: 'duplex', portId: 'to_os', peaks: [0, 1] },
    ],
    protocolVersion: 1,
    revision: 3,
  });
  const view = render(
    <MixRelayProvider>
      <AudioMeterProvider store={store}>
        <AudioNodeCard
          category="Virtual input"
          icon="IN"
          inputCount={0}
          outputCount={2}
          meterBanks={[{ channels: 2, nodeId: 'duplex', portId: 'from_os' }]}
          title="Duplex"
          onSelect={vi.fn()}
        />
        <AudioNodeCard
          category="Virtual output"
          icon="OUT"
          inputCount={2}
          outputCount={0}
          meterBanks={[{ channels: 2, nodeId: 'duplex', portId: 'to_os' }]}
          title="Duplex"
          onSelect={vi.fn()}
        />
      </AudioMeterProvider>
    </MixRelayProvider>,
  );
  const source = view.container.querySelector('[data-meter-port="from_os"]');
  const sink = view.container.querySelector('[data-meter-port="to_os"]');
  expect(source?.querySelectorAll('[data-orientation="horizontal"]')).toHaveLength(2);
  expect(sink?.querySelectorAll('[data-orientation="horizontal"]')).toHaveLength(2);
  expect(source?.querySelectorAll('[title$=": 100%"]')).toHaveLength(1);
  expect(sink?.querySelectorAll('[title$=": 100%"]')).toHaveLength(1);
});
