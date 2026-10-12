import { useLayoutEffect, useRef } from 'react';
import { Slider, SliderThumb, SliderTrack } from 'react-aria-components';
import styles from '../theme/ComponentTheme.module.css';
import { clampParameter, isLogarithmicRange, steppedParameterKey } from './parameterValue';

/** Core slider behavior and hit area, shared by levels and node parameters. */
export function ValueSlider({
  label,
  value,
  min,
  max,
  step = 1,
  disabled,
  onChange,
  valueText,
  scale = 'linear',
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
  valueText?: string;
  scale?: 'linear' | 'logarithmic';
  onChange: (value: number) => void;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const logarithmic = isLogarithmicRange({ minimum: min, maximum: max, step, scale });
  // The maximum derivative defines enough positions to preserve the public parameter step.
  const resolution = logarithmic ? Math.ceil((Math.log(max / min) * max) / step) : 1;
  const toSlider = (next: number) =>
    logarithmic ? (Math.log(next / min) / Math.log(max / min)) * resolution : next;
  const fromSlider = (next: number) =>
    logarithmic
      ? Math.max(
          min,
          Math.min(max, Math.round((min * (max / min) ** (next / resolution)) / step) * step),
        )
      : next;
  useLayoutEffect(() => {
    if (valueText) inputRef.current?.setAttribute('aria-valuetext', valueText);
    else inputRef.current?.removeAttribute('aria-valuetext');
    if (logarithmic) {
      inputRef.current?.setAttribute('aria-valuemin', String(min));
      inputRef.current?.setAttribute('aria-valuemax', String(max));
      inputRef.current?.setAttribute('aria-valuenow', String(value));
    } else {
      for (const attribute of ['aria-valuemin', 'aria-valuemax', 'aria-valuenow'])
        inputRef.current?.removeAttribute(attribute);
    }
  }, [valueText, logarithmic, min, max, value]);
  return (
    <div
      onKeyDownCapture={(event) => {
        if (!logarithmic || disabled) return;
        const next = steppedParameterKey(event.key, value, { minimum: min, maximum: max, step });
        if (next === undefined) return;
        event.preventDefault();
        event.stopPropagation();
        onChange(clampParameter(next, { minimum: min, maximum: max, step }));
      }}
    >
      <Slider
        className={`${styles.sliderRoot} nodrag nopan nowheel`}
        aria-label={label}
        value={toSlider(value)}
        minValue={logarithmic ? 0 : min}
        maxValue={logarithmic ? resolution : max}
        step={logarithmic ? 1 : step}
        isDisabled={disabled}
        onChange={(next) => onChange(fromSlider(next))}
      >
        <SliderTrack className={styles.sliderTrack}>
          {({ state }) => (
            <>
              <span
                className={styles.sliderBar}
                style={{ width: `${state.getThumbPercent(0) * 100}%` }}
              />
              {/* valueText is the announced value (aria-valuetext), not a second description. */}
              <SliderThumb inputRef={inputRef} className={styles.sliderThumb} />
            </>
          )}
        </SliderTrack>
      </Slider>
    </div>
  );
}
