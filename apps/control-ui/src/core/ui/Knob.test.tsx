import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { expect, it, vi } from 'vitest';
import { Knob, type KnobProps } from './Knob';

const defaults: KnobProps = {
  label: 'Gain',
  value: 0,
  min: -24,
  max: 24,
  step: 0.1,
  onChange: () => {},
};
function renderKnob(props: Partial<KnobProps> = {}) {
  const change = vi.fn();
  const view = render(<Knob {...defaults} {...props} onChange={change} />);
  const slider = screen.getByRole('slider');
  const dial = slider.parentElement as HTMLElement;
  dial.setPointerCapture = vi.fn();
  dial.hasPointerCapture = vi.fn(() => true);
  dial.releasePointerCapture = vi.fn();
  return { ...view, change, slider, dial };
}
it('exposes the actual value and unit without an internal integer position range', () => {
  const { slider } = renderKnob({
    value: Math.SQRT1_2,
    min: 0.1,
    max: 10,
    step: 0.01,
    valueText: '0.707 Q',
  });
  expect(slider).toHaveAttribute('aria-valuenow', String(Math.SQRT1_2));
  expect(slider).toHaveAttribute('min', '0.1');
  expect(slider).toHaveAttribute('max', '10');
  expect(slider).toHaveAttribute('aria-valuetext', '0.707 Q');
  expect(slider).toHaveAttribute('step', 'any');
});
it('supports arrows, page keys and endpoints in actual units with no graph shortcut propagation', () => {
  const { change, slider } = renderKnob();
  for (const [key, value] of [
    ['ArrowUp', 0.1],
    ['ArrowRight', 0.1],
    ['ArrowDown', -0.1],
    ['ArrowLeft', -0.1],
    ['PageUp', 1],
    ['PageDown', -1],
    ['Home', -24],
    ['End', 24],
  ] as const) {
    fireEvent.keyDown(slider, { key });
    expect(change).toHaveBeenLastCalledWith(value);
  }
  fireEvent.keyDown(slider, { key: 'Tab' });
  expect(change).toHaveBeenCalledTimes(8);
});
it('uses continuous drag position, fractional movements and Shift fine adjustment', () => {
  const { dial, change, slider } = renderKnob();
  fireEvent.pointerDown(dial, { button: 0, pointerId: 1, clientY: 100 });
  expect(slider).toHaveFocus();
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 99.5 });
  expect(change).toHaveBeenLastCalledWith(0.2);
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 99, shiftKey: true });
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 98.5, shiftKey: true });
  expect(change).toHaveBeenLastCalledWith(0.2);
  fireEvent.pointerMove(dial, { pointerId: 2, clientY: -1000 });
  expect(change).toHaveBeenCalledTimes(3);
  fireEvent.pointerCancel(dial, { pointerId: 1 });
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 0 });
  expect(change).toHaveBeenCalledTimes(3);
  expect(dial.releasePointerCapture).toHaveBeenCalledWith(1);
});
it('maps logarithmic frequency directly and keeps 1 Hz keyboard resolution', () => {
  const { change, slider, dial } = renderKnob({
    value: 20,
    min: 20,
    max: 20000,
    step: 1,
    scale: 'logarithmic',
  });
  fireEvent.pointerDown(dial, { button: 0, pointerId: 1, clientY: 100 });
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 20 });
  expect(change).toHaveBeenLastCalledWith(632);
  fireEvent.pointerUp(dial, { pointerId: 1 });
  fireEvent.keyDown(slider, { key: 'ArrowUp' });
  expect(change).toHaveBeenLastCalledWith(21);
});
it('keeps fine frequency drag at most 1 Hz per pixel even at the upper end', () => {
  const { change, dial } = renderKnob({
    value: 19900,
    min: 20,
    max: 20000,
    step: 1,
    scale: 'logarithmic',
  });
  fireEvent.pointerDown(dial, { button: 0, pointerId: 1, clientY: 100 });
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 99, shiftKey: true });
  expect(change).toHaveBeenLastCalledWith(19901);
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 98.75, shiftKey: true });
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 98.5, shiftKey: true });
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 98, shiftKey: true });
  expect(change).toHaveBeenLastCalledWith(19902);
});
it('ignores disabled/right/second pointer, cancels capture when disabled and unmounted', () => {
  const { dial, change, rerender, unmount } = renderKnob();
  fireEvent.pointerDown(dial, { button: 2, pointerId: 1, clientY: 100 });
  expect(dial.setPointerCapture).not.toHaveBeenCalled();
  fireEvent.pointerDown(dial, { button: 0, pointerId: 1, clientY: 100 });
  fireEvent.pointerDown(dial, { button: 0, pointerId: 2, clientY: 100 });
  expect(dial.setPointerCapture).toHaveBeenCalledTimes(1);
  rerender(<Knob {...defaults} disabled onChange={change} />);
  expect(dial.releasePointerCapture).toHaveBeenCalledWith(1);
  expect(screen.getByRole('slider')).toBeDisabled();
  fireEvent.pointerDown(dial, { button: 0, pointerId: 3, clientY: 100 });
  expect(dial.setPointerCapture).toHaveBeenCalledTimes(1);
  rerender(<Knob {...defaults} onChange={change} />);
  fireEvent.pointerDown(dial, { button: 0, pointerId: 4, clientY: 100 });
  unmount();
  expect(dial.releasePointerCapture).toHaveBeenLastCalledWith(4);
});
it('renders vector geometry and tracks controlled value changes', () => {
  function Demo() {
    const [value, setValue] = useState(0);
    return <Knob {...defaults} value={value} onChange={setValue} />;
  }
  render(<Demo />);
  const slider = screen.getByRole('slider');
  expect(slider.parentElement?.querySelector('svg')).toHaveAttribute('viewBox', '0 0 48 48');
  const focusArc = slider.parentElement?.querySelector('path[vector-effect]');
  expect(focusArc).toHaveAttribute('vector-effect', 'non-scaling-stroke');
  expect(focusArc).toHaveAttribute('d', 'M7.737 40.263 A23 23 0 1 1 40.263 40.263');
  fireEvent.change(slider, { target: { value: '23.7' } });
  expect(slider).toHaveAttribute('aria-valuenow', '23.7');
  fireEvent.keyDown(slider, { key: 'End' });
  fireEvent.keyDown(slider, { key: 'ArrowUp' });
  expect(slider).toHaveAttribute('aria-valuenow', '24');
});
it('does not execute keys while disabled and defaults to whole steps for a unipolar dial', () => {
  const { slider, change, rerender } = renderKnob({
    value: -5,
    min: -10,
    max: 0,
    step: undefined,
    disabled: true,
  });
  fireEvent.keyDown(slider, { key: 'ArrowUp' });
  expect(change).not.toHaveBeenCalled();
  expect(slider.parentElement?.querySelectorAll('line')).toHaveLength(1);
  rerender(<Knob {...defaults} value={-5} min={-10} max={0} step={undefined} onChange={change} />);
  fireEvent.keyDown(slider, { key: 'ArrowUp' });
  expect(change).toHaveBeenLastCalledWith(-4);
});
it('keeps an active drag when a different pointer ends, and stops after capture loss', () => {
  const { dial, change } = renderKnob();
  fireEvent.pointerDown(dial, { button: 0, pointerId: 1, clientY: 100 });
  fireEvent.pointerUp(dial, { pointerId: 2 });
  fireEvent.pointerCancel(dial, { pointerId: 2 });
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 90 });
  expect(change).toHaveBeenLastCalledWith(3);
  fireEvent.lostPointerCapture(dial, { pointerId: 1 });
  fireEvent.pointerMove(dial, { pointerId: 1, clientY: 80 });
  expect(change).toHaveBeenCalledTimes(1);
});
it('exposes channel endpoints without adding duplicate focus targets', () => {
  const { dial, slider } = renderKnob({
    value: 0,
    min: -1,
    max: 1,
    step: 0.01,
    endpoints: { minimum: 'L', maximum: 'R' },
    valueText: '0 (L -1 / R 1)',
  });
  expect(slider).toHaveAccessibleName('Gain');
  expect(slider).toHaveAttribute('aria-valuetext', '0 (L -1 / R 1)');
  expect(screen.getByText('L').parentElement).toHaveAttribute('aria-hidden', 'true');
  expect(screen.getByText('R')).toBeInTheDocument();
  expect(dial.querySelectorAll('input')).toHaveLength(1);
});
