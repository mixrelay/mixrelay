import { Button, Popover } from '@mantine/core';
import type { PropsWithChildren } from 'react';

export function InfoPopover({
  children,
  label,
  critical = false,
}: PropsWithChildren<{
  label: string;
  critical?: boolean;
}>) {
  return (
    <Popover position="top-end" width={340} withArrow withinPortal>
      <Popover.Target>
        <Button color={critical ? 'red' : 'gray'} size="compact-xs" variant="subtle">
          {label}
        </Button>
      </Popover.Target>
      <Popover.Dropdown>{children}</Popover.Dropdown>
    </Popover>
  );
}
