import styles from './SidebarToggle.module.css';
import { Tooltip } from './Tooltip';

export function SidebarToggle({
  label,
  opened,
  controls,
  onToggle,
}: {
  label: string;
  opened: boolean;
  controls: string;
  onToggle: () => void;
}) {
  return (
    <Tooltip label={label} position="right">
      <button
        className={opened ? styles.sidebarBoundary : styles.sidebarReopen}
        type="button"
        aria-label={label}
        aria-expanded={opened}
        aria-controls={controls}
        onClick={onToggle}
      >
        {!opened ? (
          <svg aria-hidden="true" viewBox="0 0 16 16">
            <path d="m6 4 4 4-4 4" />
          </svg>
        ) : null}
      </button>
    </Tooltip>
  );
}
