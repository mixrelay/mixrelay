import { Modal } from '@mantine/core';
import type { ReactNode } from 'react';

export function DriverDialog({ children, title, closeLabel, opened, onClose }: {
  children: ReactNode;
  title: string;
  closeLabel: string;
  opened: boolean;
  onClose: () => void;
}) {
  return <Modal opened={opened} onClose={onClose} title={title} size="md" centered
    closeButtonProps={{ 'aria-label': closeLabel }}>{children}</Modal>;
}
