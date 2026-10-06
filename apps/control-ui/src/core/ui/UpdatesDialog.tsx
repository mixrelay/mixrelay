import { Modal } from '@mantine/core';
import type { ReactNode } from 'react';

export function UpdatesDialog({
  children,
  closeLabel,
  onClose,
  opened,
  title,
}: {
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
      onClose={onClose}
      opened={opened}
      size="sm"
      title={title}
    >
      {children}
    </Modal>
  );
}
