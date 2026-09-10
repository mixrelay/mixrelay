import { fireEvent, render } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { NodeSignalControls } from './NodeSignalControls';

it('exposes distinct icon-only power and mute with no native tooltips', () => {
  const onPower = vi.fn();
  const onMute = vi.fn();
  const view = render(
    <NodeSignalControls
      enabled
      muted={false}
      disabled={false}
      power
      mute
      powerLabel="Disable Gain"
      muteLabel="Mute Gain"
      onPower={onPower}
      onMute={onMute}
    />,
  );
  const power = view.getByRole('button', { name: 'Disable Gain' });
  const mute = view.getByRole('button', { name: 'Mute Gain' });
  expect(power).toHaveAttribute('aria-pressed', 'true');
  expect(mute).toHaveAttribute('aria-pressed', 'false');
  expect(view.container.querySelector('[title]')).toBeNull();
  fireEvent.click(power);
  fireEvent.click(mute);
  expect(onPower).toHaveBeenCalledTimes(1);
  expect(onMute).toHaveBeenCalledTimes(1);
});

it('prevents clicks while a command is awaiting acknowledgement', () => {
  const onPower = vi.fn();
  const view = render(
    <NodeSignalControls
      enabled={false}
      muted
      disabled
      power
      mute
      powerLabel="Enable Gain"
      muteLabel="Unmute Gain"
      onPower={onPower}
      onMute={vi.fn()}
    />,
  );
  fireEvent.click(view.getByRole('button', { name: 'Enable Gain' }));
  expect(onPower).not.toHaveBeenCalled();
});
