import { Button } from 'react-aria-components';
import type { StatusTone } from './StatusBadge';
import { Tooltip } from './Tooltip';
import styles from './TransportButton.module.css';

export function TransportButton({
  icon,
  label,
  active = false,
  disabled,
  onClick,
  tooltip,
  description,
  busy,
  pressed,
  tone,
  compact = false,
}: {
  icon: 'record' | 'pause' | 'stop' | 'resume';
  label: string;
  active?: boolean;
  disabled: boolean;
  onClick: () => void;
  tooltip?: string;
  description?: string;
  busy?: boolean;
  pressed?: boolean;
  tone?: StatusTone;
  compact?: boolean;
}) {
  return (
    <Tooltip label={tooltip ?? label}>
      <Button
        className={styles.root}
        render={(props) => <button {...props} aria-busy={busy} aria-description={description} />}
        aria-label={label}
        aria-pressed={pressed ?? active}
        aria-busy={busy}
        aria-description={description}
        data-tone={tone}
        data-transport={icon}
        data-active={active || undefined}
        isDisabled={disabled}
        onClick={onClick}
        data-compact={compact}
      >
        <svg aria-hidden="true" viewBox="0 0 16 16" width="16" height="16" fill="currentColor">
          {icon === 'record' ? <circle cx="8" cy="8" r="5" /> : null}
          {icon === 'pause' ? (
            <>
              <rect x="3" y="3" width="3" height="10" />
              <rect x="10" y="3" width="3" height="10" />
            </>
          ) : null}
          {icon === 'stop' ? <rect x="3" y="3" width="10" height="10" /> : null}
          {icon === 'resume' ? <path d="M4 2 13 8 4 14Z" /> : null}
        </svg>
      </Button>
    </Tooltip>
  );
}
