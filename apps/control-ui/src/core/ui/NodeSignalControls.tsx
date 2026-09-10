import styles from './NodeSignalControls.module.css';

export function NodeSignalControls(props: {
  enabled: boolean;
  muted: boolean;
  disabled: boolean;
  power: boolean;
  mute: boolean;
  powerLabel: string;
  muteLabel: string;
  onPower: () => void;
  onMute: () => void;
}) {
  return (
    <div className={`${styles.root} nodrag nopan`}>
      {props.power ? (
        <button
          type="button"
          className={styles.button}
          aria-label={props.powerLabel}
          aria-pressed={props.enabled}
          data-action="power"
          disabled={props.disabled}
          onClick={props.onPower}
        >
          <svg aria-hidden="true" viewBox="0 0 20 20">
            <path d="M10 2v8M5 4.5a7 7 0 1 0 10 0" />
          </svg>
        </button>
      ) : null}
      {props.mute ? (
        <button
          type="button"
          className={styles.button}
          aria-label={props.muteLabel}
          aria-pressed={props.muted}
          data-action="mute"
          disabled={props.disabled}
          onClick={props.onMute}
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
      ) : null}
    </div>
  );
}
