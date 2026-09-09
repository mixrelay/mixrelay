import { Tooltip } from '@mantine/core';
import type { ReactElement } from 'react';
import { formatPlatformShortcut } from './platformShortcut';

export interface ShortcutTooltipProps {
  children: ReactElement;
  shortcut: string;
}

export function ShortcutTooltip({ children, shortcut }: ShortcutTooltipProps) {
  return (
    <Tooltip label={formatPlatformShortcut(shortcut)} openDelay={350} position="right">
      {children}
    </Tooltip>
  );
}
