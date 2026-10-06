import { fireEvent, render } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import type { NativeDeviceFormats } from '../control-port/types';
import { MixRelayProvider } from '../theme/MixRelayProvider';
import { AudioNodeCard, type EndpointControl } from './AudioNodeCard';

it('keeps free native format controls usable off graph and preserves options after an error', () => {
  const formats: NativeDeviceFormats = {
    render: { current: { sampleRate: 48000, channels: 2, sampleFormat: 'float32' }, supported: [
      { sampleRate: 48000, channels: 2, sampleFormat: 'float32' },
      { sampleRate: 48000, channels: 2, sampleFormat: 'signed32' },
      { sampleRate: 44100, channels: 2, sampleFormat: 'signed24' },
    ] },
    capture: { current: null, supported: [] },
  };
  const control: EndpointControl = {
    displayName: 'Monitor', nameLabel: 'Device name', renderChannels: 2, captureChannels: 2,
    renderChannelsLabel: 'Playback channels', captureChannelsLabel: 'Capture channels',
    nativeFormats: formats, nativeFormatLabels: { render: 'Playback format', capture: 'Capture format', unknown: 'Unknown' },
  };
  const onEndpointChange = vi.fn();
  const card = (endpointControl: EndpointControl) => <MixRelayProvider><AudioNodeCard
    category="Device" icon="IO" title="Monitor" inputCount={2} outputCount={2}
    poweredOff endpointControl={endpointControl} onEndpointChange={onEndpointChange} onSelect={vi.fn()} />
  </MixRelayProvider>;
  const view = render(card(control));
  fireEvent.click(view.getByRole('button', { name: 'Options' }));
  const playback = view.getByRole('combobox', { name: 'Playback format' });
  expect(playback).toBeEnabled();
  expect(view.getByRole('textbox', { name: 'Capture format' })).toHaveValue('Unknown');
  fireEvent.click(playback);
  expect(view.getByRole('option', { name: '48 kHz · 2 ch · 32-bit Float' })).toBeInTheDocument();
  fireEvent.click(view.getByRole('option', { name: '48 kHz · 2 ch · 32-bit PCM' }));
  expect(onEndpointChange).toHaveBeenCalledWith({ displayName: 'Monitor', renderChannels: 2, captureChannels: 2,
    nativeFormat: { direction: 'render', format: { sampleRate: 48000, channels: 2, sampleFormat: 'signed32' } } });
  view.rerender(card({ ...control, formatError: 'Refresh and try again.' }));
  expect(view.getByRole('combobox', { name: 'Playback format' })).toHaveValue('48 kHz · 2 ch · 32-bit Float');
  expect(view.getByRole('alert')).toHaveTextContent('Refresh and try again.');
  expect(view.getByRole('button', { name: 'Options' })).toHaveAttribute('aria-expanded', 'true');
});
