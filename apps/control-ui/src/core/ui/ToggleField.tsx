import { Switch } from '@mantine/core';

interface ToggleFieldProps {
  checked: boolean;
  label: string;
  onChange: (checked: boolean) => void;
}

export function ToggleField({ checked, label, onChange }: ToggleFieldProps) {
  return (
    <Switch
      checked={checked}
      color="signal"
      label={label}
      onChange={(event) => onChange(event.currentTarget.checked)}
      size="sm"
    />
  );
}
