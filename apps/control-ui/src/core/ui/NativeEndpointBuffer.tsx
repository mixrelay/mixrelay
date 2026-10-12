import styles from './AudioNodeCard.module.css';
import { SelectField } from './SelectField';

/** Actual I/O readback from the Driver runtime that owns this endpoint. */
export interface EndpointBufferControl {
  label: string;
  framesLabel: string;
  currentFrames: number | null;
  preferredFrames: number | null;
  unselectedLabel: string;
  pendingLabel: string;
  observedLabel: string;
  options: readonly number[];
  disabled: boolean;
  error?: string | null;
}

export function NativeEndpointBuffer({
  control,
  onChange,
}: {
  control: EndpointBufferControl;
  onChange: (frames: number) => void;
}) {
  const selected = control.preferredFrames ?? control.currentFrames;
  return (
    <div>
      <SelectField
        label={control.label}
        error={control.error}
        data={[...new Set([...control.options, ...(selected === null ? [] : [selected])])].map(
          (frames) => ({
            value: String(frames),
            label: `${frames} ${control.framesLabel}`,
            disabled: !control.options.includes(frames),
          }),
        )}
        disabled={control.disabled || control.options.length === 0}
        value={selected === null ? null : String(selected)}
        placeholder={control.unselectedLabel}
        onChange={(value) => onChange(Number(value))}
      />
      {control.currentFrames === null ? (
        <span className={styles.endpointBufferNotice} role="status">
          {control.pendingLabel}
        </span>
      ) : control.currentFrames !== selected ? (
        <span className={styles.endpointBufferNotice} role="status">
          {control.observedLabel}: {control.currentFrames} {control.framesLabel}
        </span>
      ) : null}
    </div>
  );
}
