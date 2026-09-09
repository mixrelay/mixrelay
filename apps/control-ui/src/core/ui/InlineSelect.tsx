import { useId } from 'react';
import styles from './InlineSelect.module.css';

export function InlineSelect({
  label,
  value,
  data,
  disabled,
  onChange,
  title,
}: {
  label: string;
  value: string;
  data: { value: string; label: string }[];
  disabled?: boolean;
  title?: string;
  onChange: (value: string) => void;
}) {
  const id = useId();
  return (
    <label className={styles.field} htmlFor={id} title={title}>
      <span>{label}</span>
      <select
        id={id}
        aria-label={label}
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.value)}
      >
        {data.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}
