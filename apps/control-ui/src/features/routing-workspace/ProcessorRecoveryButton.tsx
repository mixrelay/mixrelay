import styles from './ProcessorRecoveryButton.module.css';

export function ProcessorRecoveryButton(props: {
  disabled: boolean;
  label: string;
  retrying: boolean;
  onRetry: () => void;
}) {
  return (
    <button
      aria-label={props.label}
      aria-busy={props.retrying}
      className={`${styles.button} nodrag nopan`}
      data-action="processor-retry"
      data-tone={props.retrying ? 'warning' : 'critical'}
      disabled={props.disabled}
      type="button"
      onClick={(event) => {
        event.stopPropagation();
        props.onRetry();
      }}
    >
      <svg aria-hidden="true" viewBox="0 0 20 20">
        <path d="M16 6V2m0 4h-4M16 6a7 7 0 1 0 1.7 7.6" />
      </svg>
    </button>
  );
}
