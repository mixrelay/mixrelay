import { type PointerEvent, useCallback, useEffect, useRef } from 'react';

/** Continuous parameter range shared by Knob, ValueSlider and NumericValueControl. */
export interface ParameterRange {
  minimum: number;
  maximum: number;
  step: number;
  scale?: 'linear' | 'logarithmic';
}

/** A logarithmic scale needs a positive, non-empty range; otherwise it is linear. */
export function isLogarithmicRange({ minimum, maximum, scale }: ParameterRange) {
  return scale === 'logarithmic' && minimum > 0 && maximum > minimum;
}

export function clampParameter(value: number, { minimum, maximum }: ParameterRange) {
  return Math.max(minimum, Math.min(maximum, value));
}

/** Rounds to the public step counted from the minimum, drops float noise, then clamps. */
export function snapParameter(value: number, range: ParameterRange) {
  const { minimum, step } = range;
  return clampParameter(
    Number((minimum + Math.round((value - minimum) / step) * step).toPrecision(12)),
    range,
  );
}

/** Position 0–1 along the control, logarithmic where the range allows it. */
export function parameterPosition(value: number, range: ParameterRange) {
  const { minimum, maximum } = range;
  const clamped = clampParameter(value, range);
  return isLogarithmicRange(range)
    ? Math.log(clamped / minimum) / Math.log(maximum / minimum)
    : (clamped - minimum) / (maximum - minimum);
}

/** Unsnapped value at a 0–1 position; callers snap only when they report a value. */
export function parameterAtPosition(position: number, range: ParameterRange) {
  const { minimum, maximum } = range;
  return isLogarithmicRange(range)
    ? minimum * (maximum / minimum) ** position
    : minimum + (maximum - minimum) * position;
}

/** Keyboard contract: arrows move one public step, Page Up/Down ten, Home/End the ends.
 * Text entry keeps Left/Right for caret movement by passing `vertical`. */
export function steppedParameterKey(
  key: string,
  value: number,
  { minimum, maximum, step }: ParameterRange,
  arrows: 'both' | 'vertical' = 'both',
): number | undefined {
  const changes: Record<string, number> = {
    ArrowUp: value + step,
    ArrowDown: value - step,
    PageUp: value + step * 10,
    PageDown: value - step * 10,
    Home: minimum,
    End: maximum,
  };
  if (arrows === 'both') {
    changes.ArrowRight = value + step;
    changes.ArrowLeft = value - step;
  }
  return changes[key];
}

/** Pointer-captured vertical drag. Capture is released on pointer up/cancel, loss of
 * capture, disabling and unmount; no window listeners or timers are created. */
export function useCapturedPointerDrag<State>({
  disabled,
  begin,
  update,
}: {
  disabled?: boolean;
  /** Return undefined to ignore the press. */
  begin: (event: PointerEvent<HTMLDivElement>) => State | undefined;
  update: (state: State, event: PointerEvent<HTMLDivElement>) => void;
}) {
  const drag = useRef<{ pointer: number; element: HTMLDivElement; state: State } | null>(null);
  const finish = useCallback(() => {
    const active = drag.current;
    drag.current = null;
    if (active?.element.hasPointerCapture?.(active.pointer))
      active.element.releasePointerCapture(active.pointer);
  }, []);
  useEffect(() => {
    if (disabled) finish();
    return finish;
  }, [disabled, finish]);
  const end = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointer === event.pointerId) finish();
  };
  return {
    onPointerDown(event: PointerEvent<HTMLDivElement>) {
      if (disabled || event.button !== 0 || drag.current) return;
      const state = begin(event);
      if (state === undefined) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { pointer: event.pointerId, element: event.currentTarget, state };
    },
    onPointerMove(event: PointerEvent<HTMLDivElement>) {
      const active = drag.current;
      if (active?.pointer === event.pointerId) update(active.state, event);
    },
    onPointerUp: end,
    onPointerCancel: end,
    onLostPointerCapture: finish,
  };
}
