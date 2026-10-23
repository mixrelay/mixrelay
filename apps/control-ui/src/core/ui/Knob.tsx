import { type KeyboardEvent, useRef } from 'react';
import styles from './Knob.module.css';
import {
  isLogarithmicRange,
  parameterAtPosition,
  parameterPosition,
  snapParameter,
  steppedParameterKey,
  useCapturedPointerDrag,
} from './parameterValue';

export interface KnobProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  scale?: 'linear' | 'logarithmic';
  disabled?: boolean;
  valueText?: string;
  endpoints?: { minimum: string; maximum: string };
  onChange: (value: number) => void;
}

function useKnobInteraction({
  value,
  min,
  max,
  step = 1,
  scale = 'linear',
  disabled,
  onChange,
}: KnobProps) {
  const input = useRef<HTMLInputElement>(null);
  const range = { minimum: min, maximum: max, step, scale };
  const normalize = (next: number) => snapParameter(next, range);
  // Shift drags one public step (or less) per pixel, also at the top of log ranges.
  const finePosition = (current: number, pixels: number) => {
    const rawValue = parameterAtPosition(current, range);
    const sensitivity = isLogarithmicRange(range)
      ? (rawValue * Math.log(max / min)) / 1600
      : (max - min) / 1600;
    return parameterPosition(rawValue + pixels * Math.min(step, sensitivity), range);
  };
  const pointer = useCapturedPointerDrag({
    disabled,
    begin(event) {
      event.preventDefault();
      input.current?.focus();
      return { y: event.clientY, position: parameterPosition(value, range) };
    },
    update(active, event) {
      // The continuous position accumulates; only reported values snap to the step.
      active.position = Math.max(
        0,
        Math.min(
          1,
          event.shiftKey
            ? finePosition(active.position, active.y - event.clientY)
            : active.position + (active.y - event.clientY) / 160,
        ),
      );
      active.y = event.clientY;
      onChange(normalize(parameterAtPosition(active.position, range)));
    },
  });
  const key = (event: KeyboardEvent<HTMLInputElement>) => {
    if (disabled) return;
    const next = steppedParameterKey(event.key, value, range);
    if (next === undefined) return;
    event.preventDefault();
    event.stopPropagation();
    onChange(normalize(next));
  };
  return { input, fraction: parameterPosition(value, range), pointer, key, normalize };
}

/** A vector dial over a native range input. Drag up to increase; Shift makes fine adjustments. */
export function Knob(props: KnobProps) {
  const { label, value, min, max, disabled, valueText, endpoints, onChange } = props;
  const { input, fraction, pointer, key, normalize } = useKnobInteraction(props);
  const bipolar = min < 0 && max > 0;
  // Each bipolar half uses its own range so zero stays at twelve o'clock.
  const position = bipolar ? 0.5 + (value / (value < 0 ? -min : max)) * 0.5 : fraction;
  const origin = bipolar ? 0.5 : 0;
  const angle = -135 + position * 270;
  return (
    <div
      className={`${styles.root} nodrag nopan nowheel nokey`}
      data-disabled={disabled || undefined}
      {...pointer}
    >
      <svg className={styles.dial} viewBox="0 0 48 48" aria-hidden="true">
        <path
          className={styles.focusArc}
          d="M7.737 40.263 A23 23 0 1 1 40.263 40.263"
          vectorEffect="non-scaling-stroke"
        />
        <path
          className={styles.track}
          d="M 9.858 38.142 A 20 20 0 1 1 38.142 38.142"
          pathLength="1"
          vectorEffect="non-scaling-stroke"
        />
        <path
          className={styles.value}
          d="M 9.858 38.142 A 20 20 0 1 1 38.142 38.142"
          pathLength="1"
          strokeDasharray={`${Math.abs(position - origin)} 1`}
          strokeDashoffset={-Math.min(position, origin)}
          visibility={position === origin ? 'hidden' : undefined}
          vectorEffect="non-scaling-stroke"
        />
        {bipolar ? (
          <line
            className={styles.center}
            x1="24"
            y1="8"
            x2="24"
            y2="10"
            vectorEffect="non-scaling-stroke"
          />
        ) : null}
        <line
          className={styles.indicator}
          x1="24"
          y1="5"
          x2="24"
          y2="21"
          transform={`rotate(${angle} 24 24)`}
          vectorEffect="non-scaling-stroke"
        />
      </svg>
      {endpoints ? (
        <span className={styles.endpoints} aria-hidden="true">
          <span>{endpoints.minimum}</span>
          <span>{endpoints.maximum}</span>
        </span>
      ) : null}
      <input
        ref={input}
        className={styles.input}
        type="range"
        aria-label={label}
        aria-valuetext={valueText}
        aria-valuenow={value}
        min={min}
        max={max}
        step="any"
        value={value}
        disabled={disabled}
        onKeyDown={key}
        onChange={(event) => onChange(normalize(Number(event.target.value)))}
      />
    </div>
  );
}
