import { Tooltip } from '@mantine/core';
import type { StatusTone } from './StatusBadge';

/** The tooltip and accessible name describe exactly the same status as the colored marker. */
export function StatusIndicator({
  className,
  label,
  tone,
}: {
  className?: string;
  label: string;
  tone: StatusTone;
}) {
  return (
    <Tooltip label={label} openDelay={350} position="top">
      <span className={className} data-tone={tone} role="img" aria-label={label} />
    </Tooltip>
  );
}
