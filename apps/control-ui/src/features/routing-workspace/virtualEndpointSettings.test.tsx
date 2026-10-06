import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { AudioNodeCard } from '../../core/ui/AudioNodeCard';
import { virtualPolicy, virtualSpec } from './virtualEndpointFixtures';

it.each(['Render channels', 'Capture channels'])(
  'offers exactly the advertised backend options through 64 for %s',
  (label) => {
    const onChange = vi.fn();
    render(
      <MixRelayProvider>
        <AudioNodeCard
          category="Device"
          icon="IO"
          title={virtualSpec.displayName}
          inputCount={16}
          outputCount={64}
          onSelect={vi.fn()}
          optionsLabel="Options"
          onEndpointChange={onChange}
          endpointControl={{
            displayName: virtualSpec.displayName,
            nameLabel: 'Device name',
            renderChannels: 64,
            renderChannelsLabel: 'Render channels',
            captureChannels: 16,
            captureChannelsLabel: 'Capture channels',
            allowedRenderChannels: virtualPolicy.renderChannels,
            allowedCaptureChannels: virtualPolicy.captureChannels,
          }}
        />
      </MixRelayProvider>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Options' }));
    fireEvent.click(screen.getByRole('combobox', { name: label }));
    expect(screen.getAllByRole('option').map((option) => option.textContent)).toEqual(
      virtualPolicy.renderChannels.map(String),
    );
    expect(screen.queryByRole('option', { name: '65' })).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole('option', { name: label === 'Render channels' ? '32' : '64' }),
    );
    expect(onChange).toHaveBeenCalledWith({
      displayName: virtualSpec.displayName,
      renderChannels: label === 'Render channels' ? 32 : 64,
      captureChannels: label === 'Capture channels' ? 64 : 16,
    });
  },
);
