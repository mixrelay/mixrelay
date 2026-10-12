import type { StatusTone } from './StatusBadge';
import { Tooltip } from './Tooltip';

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
    <Tooltip nonInteractive label={label} position="top">
      <span className={className} data-tone={tone} role="img" aria-label={label} />
    </Tooltip>
  );
}
