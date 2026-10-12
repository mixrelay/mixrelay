import type { ReactNode } from 'react';
import { Dialog } from '../ui/Dialog';
import { TextField } from '../ui/TextField';

export function LicensingDialog({
  busy,
  children,
  closeLabel,
  onClose,
  opened,
  title,
}: {
  busy: boolean;
  children: ReactNode;
  closeLabel: string;
  onClose: () => void;
  opened: boolean;
  title: string;
}) {
  return (
    <Dialog
      closeLabel={closeLabel}
      busy={busy}
      onClose={onClose}
      opened={opened}
      size="sm"
      title={title}
      withCloseButton={!busy}
    >
      {children}
    </Dialog>
  );
}

export function LicensingKeyField({
  disabled,
  label,
  onChange,
  value,
}: {
  disabled: boolean;
  label: string;
  onChange: (value: string) => void;
  value: string;
}) {
  return (
    <TextField
      type="password"
      autoComplete="off"
      disabled={disabled}
      label={label}
      onChange={(event) => onChange(event.currentTarget.value)}
      value={value}
    />
  );
}
