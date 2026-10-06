import { Modal, PasswordInput } from '@mantine/core';
import type { ReactNode } from 'react';

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
    <Modal
      centered
      closeButtonProps={{ 'aria-label': closeLabel }}
      closeOnClickOutside={!busy}
      closeOnEscape={!busy}
      onClose={onClose}
      opened={opened}
      size="sm"
      title={title}
      withCloseButton={!busy}
    >
      {children}
    </Modal>
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
    <PasswordInput
      autoComplete="off"
      disabled={disabled}
      label={label}
      onChange={(event) => onChange(event.currentTarget.value)}
      size="xs"
      value={value}
    />
  );
}
