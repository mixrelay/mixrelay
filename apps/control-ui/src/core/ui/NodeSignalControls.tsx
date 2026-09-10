import styles from './NodeSignalControls.module.css';

export function NodePowerToggle(props: {
  enabled: boolean;
  disabled: boolean;
  label: string;
  onChange: () => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      className={`${styles.power} nodrag nopan`}
      aria-label={props.label}
      aria-checked={props.enabled}
      disabled={props.disabled}
      onClick={(event) => {
        event.stopPropagation();
        props.onChange();
      }}
    >
      <span className={styles.track} aria-hidden="true">
        <span className={styles.thumb} />
      </span>
    </button>
  );
}

export function NodeMuteButton(props: {
  muted: boolean;
  disabled: boolean;
  label: string;
  onChange: () => void;
}) {
  return (
    <button
      type="button"
      className={`${styles.button} nodrag nopan`}
      aria-label={props.label}
      aria-pressed={props.muted}
      data-action="mute"
      disabled={props.disabled}
      onClick={(event) => {
        event.stopPropagation();
        props.onChange();
      }}
    >
      <svg aria-hidden="true" viewBox="0 0 20 20">
        <path d="M3 7h3l4-4v14l-4-4H3z" />
        {props.muted ? (
          <path d="m13 7 5 6m0-6-5 6" />
        ) : (
          <path d="M13 6a6 6 0 0 1 0 8m2-11a10 10 0 0 1 0 14" />
        )}
      </svg>
    </button>
  );
}
