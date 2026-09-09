import { ActionIcon, Tooltip } from '@mantine/core';
import styles from './TransportButton.module.css';

export function TransportButton({
  icon,
  label,
  active = false,
  disabled,
  onClick,
}: {
  icon: 'record' | 'pause' | 'stop' | 'resume';
  label: string;
  active?: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <Tooltip label={label} events={{ hover: true, focus: true, touch: false }}>
      <span className={styles.target}>
        <ActionIcon
          className={styles.root}
          aria-label={label}
          aria-pressed={active}
          data-transport={icon}
          data-active={active || undefined}
          disabled={disabled}
          onClick={onClick}
          size={28}
          variant="default"
          radius={3}
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
        </ActionIcon>
      </span>
    </Tooltip>
  );
}
