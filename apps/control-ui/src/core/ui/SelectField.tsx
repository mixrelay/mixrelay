import { Select } from '@mantine/core';

export function SelectField(props: {
  label: string;
  value: string;
  data: { value: string; label: string }[];
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <Select
      {...props}
      comboboxProps={{ withinPortal: false }}
      size="xs"
      allowDeselect={false}
      onChange={(value) => {
        if (value !== null) props.onChange(value);
      }}
    />
  );
}
