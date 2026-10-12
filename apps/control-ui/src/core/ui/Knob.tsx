import { type KeyboardEvent, type PointerEvent, useCallback, useEffect, useRef } from 'react';
import styles from './Knob.module.css';

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

function nextKeyboardValue(key: string, value: number, step: number, min: number, max: number) {
  const changes: Record<string, number> = {
    ArrowUp: value + step,
    ArrowRight: value + step,
    ArrowDown: value - step,
    ArrowLeft: value - step,
    PageUp: value + step * 10,
    PageDown: value - step * 10,
    Home: min,
    End: max,
  };
  return changes[key];
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
  const drag = useRef<{
    pointer: number;
    y: number;
    position: number;
    element: HTMLElement;
  } | null>(null);
  const logarithmic = scale === 'logarithmic' && min > 0 && max > min;
  const clamp = (next: number) => Math.max(min, Math.min(max, next));
  const position = (next: number) =>
    logarithmic
      ? Math.log(clamp(next) / min) / Math.log(max / min)
      : (clamp(next) - min) / (max - min);
  const normalize = (next: number) =>
    clamp(Number((Math.round((next - min) / step) * step + min).toPrecision(12)));
  const fromPosition = (next: number) =>
    normalize(logarithmic ? min * (max / min) ** next : min + (max - min) * next);
  const finePosition = (current: number, pixels: number) => {
    const rawValue = logarithmic ? min * (max / min) ** current : min + (max - min) * current;
    const sensitivity = logarithmic ? (rawValue * Math.log(max / min)) / 1600 : (max - min) / 1600;
    return position(rawValue + pixels * Math.min(step, sensitivity));
  };
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
  const start = (event: PointerEvent<HTMLDivElement>) => {
    if (disabled || event.button !== 0 || drag.current) return;
    event.preventDefault();
    input.current?.focus();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      pointer: event.pointerId,
      y: event.clientY,
      position: position(value),
      element: event.currentTarget,
    };
  };
  const move = (event: PointerEvent<HTMLDivElement>) => {
    const active = drag.current;
    if (!active || active.pointer !== event.pointerId) return;
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
    onChange(fromPosition(active.position));
  };
  const key = (event: KeyboardEvent<HTMLInputElement>) => {
    if (disabled) return;
    const next = nextKeyboardValue(event.key, value, step, min, max);
    if (next === undefined) return;
    event.preventDefault();
    event.stopPropagation();
    onChange(normalize(next));
  };
  const end = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointer === event.pointerId) finish();
  };
  return { input, fraction: position(value), start, move, end, finish, key, normalize };
}

/** A vector dial over a native range input. Drag up to increase; Shift makes fine adjustments. */
export function Knob(props: KnobProps) {
  const { label, value, min, max, disabled, valueText, endpoints, onChange } = props;
  const { input, fraction, start, move, end, finish, key, normalize } = useKnobInteraction(props);
  const angle = -135 + fraction * 270;
  return (
    <div
      className={`${styles.root} nodrag nopan nowheel nokey`}
      data-disabled={disabled || undefined}
      onPointerDown={start}
      onPointerMove={move}
      onPointerUp={end}
      onPointerCancel={end}
      onLostPointerCapture={finish}
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
        />
        <path
          className={styles.value}
          d="M 9.858 38.142 A 20 20 0 1 1 38.142 38.142"
          pathLength="1"
          strokeDasharray={`${fraction} 1`}
        />
        <circle className={styles.body} cx="24" cy="24" r="14" />
        <line
          className={styles.indicator}
          x1="24"
          y1="13"
          x2="24"
          y2="21"
          transform={`rotate(${angle} 24 24)`}
        />
        {min < 0 && max > 0 ? (
          <line className={styles.center} x1="24" y1="0" x2="24" y2="3" />
        ) : null}
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
