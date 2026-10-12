import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { Input, NumberField } from 'react-aria-components';
import styles from './NumericValueControl.module.css';
import {
  clampParameter,
  isLogarithmicRange,
  type ParameterRange,
  snapParameter,
  steppedParameterKey,
  useCapturedPointerDrag,
} from './parameterValue';

export interface NumericValueControlProps {
  label: string;
  value: number;
  minimum: number;
  maximum: number;
  step: number;
  scale?: 'linear' | 'logarithmic';
  suffix?: string;
  density?: 'regular' | 'compact';
  disabled?: boolean;
  onChange: (value: number) => void;
  onActivate?: () => void;
}
function rangeOf(props: NumericValueControlProps): ParameterRange {
  return {
    minimum: props.minimum,
    maximum: props.maximum,
    step: props.step,
    scale: props.scale,
  };
}
function normalize(value: number, props: NumericValueControlProps) {
  return snapParameter(value, rangeOf(props));
}
function useValueDrag(props: NumericValueControlProps) {
  return useCapturedPointerDrag({
    disabled: props.disabled,
    begin(event) {
      event.currentTarget.querySelector('input')?.focus();
      return { y: event.clientY, value: props.value };
    },
    update(active, event) {
      if (active.y === event.clientY) return;
      event.preventDefault();
      event.stopPropagation();
      const pixels = active.y - event.clientY;
      // Log ranges move by a continuous ratio (240px per decade span); Shift and linear
      // ranges move in public steps, Shift at one tenth. Only the reported value snaps.
      active.value = clampParameter(
        isLogarithmicRange(rangeOf(props)) && !event.shiftKey
          ? active.value * (props.maximum / props.minimum) ** (pixels / 240)
          : active.value + pixels * props.step * (event.shiftKey ? 0.1 : 1),
        rangeOf(props),
      );
      active.y = event.clientY;
      props.onChange(normalize(active.value, props));
    },
  });
}
function useValueWheel(props: NumericValueControlProps) {
  const ref = useRef<HTMLDivElement>(null);
  const current = useRef(props);
  current.current = props;
  const remainder = useRef(0);
  const pending = useRef(props.value);
  const observed = useRef(props.value);
  if (observed.current !== props.value) {
    pending.current = props.value;
    observed.current = props.value;
    remainder.current = 0;
  }
  useEffect(() => {
    const element = ref.current as HTMLDivElement;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      event.stopPropagation();
      const options = current.current;
      if (options.disabled || event.deltaY === 0) return;
      options.onActivate?.();
      remainder.current += Math.sign(-event.deltaY) * (event.shiftKey ? 0.1 : 1);
      const wholeSteps = Math.trunc(Number(remainder.current.toPrecision(12)));
      if (!wholeSteps) return;
      remainder.current -= wholeSteps;
      pending.current = normalize(pending.current + wholeSteps * options.step, options);
      options.onChange(pending.current);
    };
    element.addEventListener('wheel', wheel, { passive: false });
    return () => element.removeEventListener('wheel', wheel);
  }, []);
  return ref;
}

/** One precise value owner with direct entry, unit-step keys/wheel and continuous drag. */
export function NumericValueControl(props: NumericValueControlProps) {
  const unitId = useId();
  const drag = useValueDrag(props);
  const ref = useValueWheel(props);
  const text = String(Number(props.value.toFixed(3)));
  const [draft, setDraft] = useState<string | null>(null);
  useLayoutEffect(() => {
    // A committed wheel/key/graph change supersedes the local editing width.
    void props.value;
    setDraft(null);
  }, [props.value]);
  return (
    <div
      ref={ref}
      className={`${styles.root} nodrag nopan nowheel nokey`}
      data-density={props.density}
      {...drag}
      onKeyDownCapture={(event) => {
        if (props.disabled) return;
        const next = steppedParameterKey(event.key, props.value, rangeOf(props), 'vertical');
        if (next === undefined) return;
        event.preventDefault();
        event.stopPropagation();
        props.onChange(normalize(next, props));
      }}
    >
      <NumberField
        aria-label={props.label}
        value={props.value}
        minValue={props.minimum}
        maxValue={props.maximum}
        step={props.step}
        isDisabled={props.disabled}
        formatOptions={{ maximumFractionDigits: 3, useGrouping: false }}
        onChange={(value) => {
          if (Number.isFinite(value)) props.onChange(value);
        }}
      >
        <div className={styles.bundle}>
          <Input
            className={styles.input}
            style={{ width: `${Math.max(1, (draft ?? text).length)}ch` }}
            aria-describedby={props.suffix ? unitId : undefined}
            onFocus={(event) => {
              setDraft(event.currentTarget.value);
              props.onActivate?.();
            }}
            onChange={(event) => setDraft(event.currentTarget.value)}
            onBlur={() => setDraft(null)}
          />
          {props.suffix ? (
            <span className={styles.unit} id={unitId}>
              {props.suffix}
            </span>
          ) : null}
        </div>
      </NumberField>
    </div>
  );
}
