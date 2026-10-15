import iconStyles from './IconActionButton.module.css';
import { Tooltip } from './Tooltip';

/**
 * One fixed header command for the graph list. It stays in place whether the list is
 * open or closed, so toggling never moves the header or covers the canvas.
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
    <Tooltip label={label}>
      <button
        className={iconStyles.root}
        type="button"
        aria-label={label}
        aria-expanded={opened}
        aria-controls={controls}
        onClick={onToggle}
      >
        <svg aria-hidden="true" viewBox="0 0 16 16">
          <rect x="2" y="3" width="12" height="10" rx="1.5" />
          <path d="M6 3v10" />
          {opened ? <path d="M3.5 4.5h2v7h-2z" fill="currentColor" stroke="none" /> : null}
        </svg>
      </button>
    </Tooltip>
  );
}
