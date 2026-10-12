import type { ReactNode } from 'react';
import styles from './ApplicationStatusBar.module.css';

/** App information and one global execution action share a fixed, single row. */
export function ApplicationStatusBar({
  information,
  controls,
  className,
}: {
  information?: ReactNode;
  controls: ReactNode;
  className?: string;
}) {
  return (
    <footer className={[styles.root, className].filter(Boolean).join(' ')}>
      <div className={styles.information}>{information}</div>
      <div className={styles.controls}>{controls}</div>
    </footer>
  );
}
