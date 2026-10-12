import styles from './ProgressIndicator.module.css';

/** Indeterminate progress: a turning ring with the same state announced as text once. */
export function ProgressIndicator({ label }: { label: string }) {
  return (
    <span className={styles.root}>
      <span className={styles.ring} aria-hidden="true" />
      <span className={styles.label} role="status">
        {label}
      </span>
    </span>
  );
}
