import { Select } from '@mantine/core';
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
  return (
    <div className={styles.field} title={title}>
      <span>{label}</span>
      <Select
        aria-label={label}
        value={value}
        data={data}
        disabled={disabled}
        className={styles.select}
        classNames={{ input: styles.input }}
        size="xs"
        rightSectionWidth={18}
        searchable={false}
        clearable={false}
        allowDeselect={false}
        withCheckIcon={false}
        maxDropdownHeight={192}
        comboboxProps={{
          withinPortal: true,
          position: 'bottom',
          middlewares: { flip: true, shift: true },
          width: 'target',
        }}
        onOptionSubmit={onChange}
      />
    </div>
  );
}
