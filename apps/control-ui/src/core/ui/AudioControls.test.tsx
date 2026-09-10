import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { ToneNariProvider } from '../theme/ToneNariProvider';
import { AudioNodeCard } from './AudioNodeCard';
import { LevelControl } from './LevelControl';
import { Meter } from './Meter';
import { Panel } from './Panel';
import { ToggleField } from './ToggleField';

it('selects a native device rate without changing its observed value optimistically', () => {
  const onChange = vi.fn();
  render(
    <ToneNariProvider>
      <AudioNodeCard
        category="Device"
        icon="IO"
        title="LAB"
        inputCount={2}
        outputCount={2}
        onSelect={vi.fn()}
        optionsLabel="Options"
        onEndpointChange={onChange}
        endpointControl={{
          displayName: 'LAB',
          nameLabel: 'Device name',
          renderChannels: 2,
          renderChannelsLabel: 'Render channels',
          captureChannels: 2,
          captureChannelsLabel: 'Capture channels',
          observedRate: { label: 'Device sample rate', value: 48000 },
          sampleRateOptions: [44100, 48000],
        }}
      />
    </ToneNariProvider>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Options' }));
  const nameField = screen.getByRole('textbox', { name: 'Device name' });
  const inputField = screen.getByRole('combobox', { name: 'Render channels' });
  const outputField = screen.getByRole('combobox', { name: 'Capture channels' });
  const rateField = screen.getByRole('combobox', { name: 'Device sample rate' });
  for (const [previous, next] of [
    [nameField, inputField],
    [inputField, outputField],
    [outputField, rateField],
  ] as const)
    expect(previous.compareDocumentPosition(next) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(inputField.closest('[class*="endpointChannels"]')).not.toBeNull();
  expect(inputField.closest('[class*="endpointChannels"]')).toBe(
    outputField.closest('[class*="endpointChannels"]'),
  );
  fireEvent.click(screen.getByRole('combobox', { name: 'Device sample rate' }));
  fireEvent.click(screen.getByRole('option', { name: '44.1 kHz' }));
  expect(onChange).toHaveBeenCalledWith({
    displayName: 'LAB',
    renderChannels: 2,
    captureChannels: 2,
    sampleRate: 44100,
  });
  expect(screen.getByRole('combobox', { name: 'Device sample rate' })).toHaveValue('48 kHz');
});

describe('Native endpoint controls', () => {
  it('renders fixed native endpoint settings read-only', () => {
    const onChange = vi.fn();
    render(
      <ToneNariProvider>
        <AudioNodeCard
          category="Device"
          icon="IO"
          title="LAB"
          inputCount={1}
          outputCount={2}
          onSelect={vi.fn()}
          optionsLabel="Options"
          onEndpointChange={onChange}
          endpointControl={{
            displayName: 'LAB',
            nameLabel: 'Device name',
            renderChannels: 2,
            renderChannelsLabel: 'Render channels',
            captureChannels: 1,
            captureChannelsLabel: 'Capture channels',
            canRename: false,
            observedRate: { label: 'Device sample rate', value: 44100 },
            allowedRenderChannels: [2],
            allowedCaptureChannels: [1],
          }}
        />
      </ToneNariProvider>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Options' }));
    expect(screen.getByLabelText('Device name')).toHaveAttribute('readonly');
    expect(screen.getByRole('combobox', { name: 'Render channels' })).toBeDisabled();
    expect(screen.getByRole('combobox', { name: 'Capture channels' })).toBeDisabled();
    expect(screen.getByLabelText('Device sample rate')).toHaveValue('44.1 kHz');
    expect(screen.getByLabelText('Device sample rate')).toHaveAttribute('readonly');
    fireEvent.blur(screen.getByLabelText('Device name'));
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe('Core audio controls', () => {
  it('forwards node, toggle, and level interactions', () => {
    const onSelect = vi.fn();
    const onToggle = vi.fn();
    const onLevel = vi.fn();

    render(
      <ToneNariProvider>
        <div>
          <AudioNodeCard
            category="Source"
            detail="Test source"
            icon="T"
            inputCount={1}
            onSelect={onSelect}
            outputCount={2}
            title="Reusable node"
          />
          <AudioNodeCard
            bypassed
            category="Output"
            detail="OBS input"
            icon="O"
            inputCount={2}
            onSelect={vi.fn()}
            outputCount={0}
            selected
            showPorts={false}
            status="Ready"
            statusTone="positive"
            title="Virtual output"
          />
          <ToggleField checked={false} label="Enabled" onChange={onToggle} />
          <LevelControl label="Level" max={10} min={0} onChange={onLevel} suffix="%" value={5} />
          <Meter label="Test" level={42} />
          <Panel title="Panel title">Panel content</Panel>
        </div>
      </ToneNariProvider>,
    );

    const node = screen.getByRole('button', { name: 'Reusable node, Source, Test source' });
    fireEvent.click(node);
    expect(onSelect).toHaveBeenCalledOnce();
    expect(within(node).getByText('Reusable node')).toBeInTheDocument();
    expect(screen.getByLabelText('Ready')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('switch', { name: 'Enabled' }));
    expect(onToggle).toHaveBeenCalledWith(true);

    fireEvent.keyDown(screen.getByRole('slider'), { key: 'ArrowRight' });
    expect(onLevel).toHaveBeenCalledWith(6);
    expect(screen.getByTitle('Test: 42%')).toBeInTheDocument();
  });

  it('offers retry only for a failed native processor editor', () => {
    const onEditorToggle = vi.fn();
    render(
      <ToneNariProvider>
        <AudioNodeCard
          category="Processor"
          icon="FX"
          inputCount={2}
          onProcessorEditorRetry={onEditorToggle}
          onSelect={vi.fn()}
          optionsLabel="Options"
          outputCount={2}
          processorEditor={{
            actionLabel: 'Retry',
            state: 'failed',
          }}
          title="Compressor"
        />
      </ToneNariProvider>,
    );

    expect(screen.queryByRole('button', { name: 'Options' })).not.toBeInTheDocument();
    expect(screen.queryByText('Plug-in editor')).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onEditorToggle).toHaveBeenCalledOnce();
  });
});
