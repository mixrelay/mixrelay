import { fireEvent, render, screen } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import {
  clampParameter,
  isLogarithmicRange,
  parameterAtPosition,
  parameterPosition,
  snapParameter,
  steppedParameterKey,
  useCapturedPointerDrag,
} from './parameterValue';

const gain = { minimum: -60, maximum: 12, step: 0.5 };
const frequency = { minimum: 20, maximum: 20_000, step: 1, scale: 'logarithmic' as const };

it('uses a logarithmic scale only for a positive, non-empty range', () => {
  expect(isLogarithmicRange(frequency)).toBe(true);
  expect(isLogarithmicRange({ ...frequency, scale: 'linear' })).toBe(false);
  expect(isLogarithmicRange({ ...frequency, minimum: 0 })).toBe(false);
  expect(isLogarithmicRange({ ...frequency, maximum: 20 })).toBe(false);
});

it('snaps to the public step from the minimum without float noise and clamps', () => {
  expect(clampParameter(99, gain)).toBe(12);
  expect(snapParameter(-5.74, gain)).toBe(-5.5);
  expect(snapParameter(0.1 + 0.2, { minimum: 0, maximum: 1, step: 0.1 })).toBe(0.3);
  expect(snapParameter(-80, gain)).toBe(-60);
});

it('maps positions linearly or logarithmically and back', () => {
  expect(parameterPosition(-24, gain)).toBe(0.5);
  expect(parameterAtPosition(0.5, gain)).toBe(-24);
  expect(parameterPosition(632.455532, frequency)).toBeCloseTo(0.5, 6);
  expect(parameterAtPosition(0.5, frequency)).toBeCloseTo(632.455532, 5);
  expect(parameterPosition(1e9, frequency)).toBe(1);
});

it('steps by one unit, ten units or to the ends, keeping Left/Right for text carets', () => {
  expect(steppedParameterKey('ArrowUp', 0, gain)).toBe(0.5);
  expect(steppedParameterKey('ArrowLeft', 0, gain)).toBe(-0.5);
  expect(steppedParameterKey('PageDown', 0, gain)).toBe(-5);
  expect(steppedParameterKey('Home', 0, gain)).toBe(-60);
  expect(steppedParameterKey('End', 0, gain)).toBe(12);
  expect(steppedParameterKey('ArrowRight', 0, gain, 'vertical')).toBeUndefined();
  expect(steppedParameterKey('ArrowDown', 0, gain, 'vertical')).toBe(-0.5);
  expect(steppedParameterKey('Enter', 0, gain)).toBeUndefined();
});

function DragProbe({
  disabled,
  begin,
  update,
}: {
  disabled?: boolean;
  begin: (y: number) => { y: number } | undefined;
  update: (state: { y: number }, y: number) => void;
}) {
  const handlers = useCapturedPointerDrag({
    disabled,
    begin: (event) => begin(event.clientY),
    update: (state, event) => update(state, event.clientY),
  });
  return <div data-testid="drag" {...handlers} />;
}

function captureSpies(element: HTMLElement) {
  const captured = new Set<number>();
  element.setPointerCapture = vi.fn((id: number) => void captured.add(id));
  element.hasPointerCapture = vi.fn((id: number) => captured.has(id));
  element.releasePointerCapture = vi.fn((id: number) => void captured.delete(id));
  return element;
}

it('captures one primary pointer, ignores others and releases on up, loss and unmount', () => {
  const begin = vi.fn((y: number) => ({ y }));
  const update = vi.fn();
  const view = render(<DragProbe begin={begin} update={update} />);
  const element = captureSpies(screen.getByTestId('drag'));

  fireEvent.pointerDown(element, { button: 2, pointerId: 1, clientY: 10 });
  expect(begin).not.toHaveBeenCalled();
  fireEvent.pointerDown(element, { button: 0, pointerId: 1, clientY: 10 });
  fireEvent.pointerDown(element, { button: 0, pointerId: 2, clientY: 10 });
  expect(begin).toHaveBeenCalledOnce();
  fireEvent.pointerMove(element, { pointerId: 2, clientY: 5 });
  fireEvent.pointerMove(element, { pointerId: 1, clientY: 4 });
  expect(update).toHaveBeenCalledExactlyOnceWith({ y: 10 }, 4);
  fireEvent.pointerUp(element, { pointerId: 2 });
  expect(element.releasePointerCapture).not.toHaveBeenCalled();
  fireEvent.pointerUp(element, { pointerId: 1 });
  expect(element.releasePointerCapture).toHaveBeenCalledWith(1);

  fireEvent.pointerDown(element, { button: 0, pointerId: 3, clientY: 0 });
  fireEvent.lostPointerCapture(element, { pointerId: 3 });
  fireEvent.pointerMove(element, { pointerId: 3, clientY: 1 });
  expect(update).toHaveBeenCalledOnce();

  fireEvent.pointerDown(element, { button: 0, pointerId: 4, clientY: 0 });
  view.unmount();
  expect(element.releasePointerCapture).toHaveBeenLastCalledWith(4);
});

it('ignores presses a control declines and ends an active drag when disabled', () => {
  const update = vi.fn();
  const view = render(<DragProbe begin={() => undefined} update={update} />);
  const element = captureSpies(screen.getByTestId('drag'));
  fireEvent.pointerDown(element, { button: 0, pointerId: 1, clientY: 0 });
  expect(element.setPointerCapture).not.toHaveBeenCalled();

  view.rerender(<DragProbe begin={(y) => ({ y })} update={update} />);
  fireEvent.pointerDown(element, { button: 0, pointerId: 5, clientY: 0 });
  view.rerender(<DragProbe disabled begin={(y) => ({ y })} update={update} />);
  expect(element.releasePointerCapture).toHaveBeenCalledWith(5);
  fireEvent.pointerDown(element, { button: 0, pointerId: 6, clientY: 0 });
  fireEvent.pointerMove(element, { pointerId: 6, clientY: 1 });
  expect(update).not.toHaveBeenCalled();
});
