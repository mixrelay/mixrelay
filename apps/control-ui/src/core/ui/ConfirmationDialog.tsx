import { Modal } from '@mantine/core';
import type { ReactNode } from 'react';
import { ActionButton } from './ActionButton';

interface ConfirmationDialogProps {
  opened: boolean;
  title: string;
  children: ReactNode;
  busy: boolean;
  disabled: boolean;
  cancelLabel: string;
  onCancel: () => void;
  onConfirm: () => void;
}

export function ConfirmationDialog(props: ConfirmationDialogProps) {
  return (
    <Modal
      opened={props.opened}
      onClose={props.onCancel}
      title={props.title}
      size="xs"
      centered
      closeOnEscape={!props.busy}
      closeOnClickOutside={!props.busy}
      withCloseButton={false}
    >
      {props.children}
      <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
        <ActionButton
          data-autofocus
          disabled={props.busy}
          variant="subtle"
          color="gray"
          onClick={props.onCancel}
        >
          {props.cancelLabel}
        </ActionButton>
        <ActionButton color="red" disabled={props.disabled} onClick={props.onConfirm}>
          {props.title}
        </ActionButton>
      </div>
    </Modal>
  );
}
