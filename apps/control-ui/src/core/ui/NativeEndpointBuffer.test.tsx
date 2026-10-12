import { fireEvent, render, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';

import { MixRelayProvider } from '../theme/MixRelayProvider';
import { type EndpointBufferControl, NativeEndpointBuffer } from './NativeEndpointBuffer';

const control: EndpointBufferControl = {
  label: 'Buffer size',
  framesLabel: 'frames',
  currentFrames: 128,
  preferredFrames: null,
  options: [128, 256, 512],
  disabled: false,
  unselectedLabel: 'Not selected',
  pendingLabel: 'Applied when the device connects.',
  observedLabel: 'Currently applied',
};

function card(value: EndpointBufferControl, onChange = vi.fn()) {
  return (
    <MixRelayProvider>
      <NativeEndpointBuffer control={value} onChange={onChange} />
    </MixRelayProvider>
  );
}

it('requests a supported value and keeps actual readback until acknowledgement', () => {
  const change = vi.fn();
  const view = render(card(control, change));
  const select = view.getByRole('button', { name: /Buffer size$/ });
  expect(select).toHaveTextContent('128 frames');
  fireEvent.click(select);
  fireEvent.click(view.getByRole('option', { name: '256 frames' }));
  expect(change).toHaveBeenCalledExactlyOnceWith(256);
  expect(select).toHaveTextContent('128 frames');
  view.rerender(card({ ...control, disabled: true }, change));
  expect(view.getByRole('button', { name: /Buffer size$/ })).toBe(select);
  expect(select).toHaveTextContent('128 frames');
  expect(select).toBeDisabled();
});

it('distinguishes the saved selection from actual I/O, including a disconnected device', () => {
  const view = render(card({ ...control, preferredFrames: 256 }));
  expect(view.getByRole('button', { name: /Buffer size$/ })).toHaveTextContent('256 frames');
  expect(view.getByRole('status')).toHaveTextContent('Currently applied: 128 frames');
  view.rerender(card({ ...control, currentFrames: null, preferredFrames: 256 }));
  expect(view.getByRole('button', { name: /Buffer size$/ })).toHaveTextContent('256 frames');
  expect(view.getByRole('status')).toHaveTextContent('Applied when the device connects.');
  view.rerender(card({ ...control, currentFrames: null }));
  expect(view.getByRole('button', { name: /Buffer size$/ })).toHaveTextContent('Not selected');
  expect(view.getByRole('button', { name: /Buffer size$/ })).toHaveTextContent('Not selected');
});

it('retains exact nonpreset readback and reports an error without closing the control', () => {
  const view = render(card({ ...control, currentFrames: 192, options: [], error: 'Try again.' }));
  expect(view.getByRole('button', { name: /Buffer size$/ })).toHaveTextContent('192 frames');
  expect(view.getByRole('button', { name: /Buffer size$/ })).toBeDisabled();
  expect(view.getByText('Try again.')).toBeInTheDocument();
});

it('supports keyboard selection and focus in the same dropdown', async () => {
  const change = vi.fn();
  const view = render(card(control, change));
  const select = view.getByRole('button', { name: /Buffer size$/ });
  select.focus();
  fireEvent.click(select);
  const option = view.getByRole('option', { name: '256 frames' });
  option.focus();
  fireEvent.keyDown(option, { key: 'Enter', code: 'Enter' });
  expect(change).toHaveBeenCalled();
  await waitFor(() => expect(select).toHaveFocus());
});

it('shows unsupported readback without submitting it as a supported choice', () => {
  const change = vi.fn();
  const view = render(card({ ...control, currentFrames: 192 }, change));
  fireEvent.click(view.getByRole('button', { name: /Buffer size$/ }));
  fireEvent.click(view.getByRole('option', { name: '192 frames' }));
  expect(change).not.toHaveBeenCalled();
});
