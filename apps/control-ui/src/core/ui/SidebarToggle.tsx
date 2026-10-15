import styles from './SidebarToggle.module.css';
import { Tooltip } from './Tooltip';

/**
 * A drawer-pull tab centred on the canvas height. Closed, it hangs on the window's left
 * edge; open, the same tab sits on the sidebar's border. It is the one sidebar control
 * and never sits in, or moves, the header.
 */
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
        className={styles.handle}
        type="button"
        aria-label={label}
        aria-expanded={opened}
        aria-controls={controls}
        onClick={onToggle}
      >
        <span className={styles.tab}>
          <svg aria-hidden="true" viewBox="0 0 16 16">
            <path d={opened ? 'm10 4-4 4 4 4' : 'm6 4 4 4-4 4'} />
          </svg>
        </span>
      </button>
    </Tooltip>
  );
}
