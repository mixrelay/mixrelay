import type { ReactNode } from 'react';
import styles from './AudioNodeCard.module.css';

/** Node utility row: a stable value at left and compact transport actions at right. */
export function NodeTransportRow({
  value,
  label,
  state,
  description,
  children,
}: {
  value: ReactNode;
  label: string;
  state?: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className={styles.transportRow}>
      <output
        className={styles.transportValue}
        aria-label={label}
        aria-live="off"
        aria-description={[state, description].filter(Boolean).join(' · ')}
        title={[state, description].filter(Boolean).join(' · ')}
      >
        {value}
      </output>
      <div className={styles.transportActions}>{children}</div>
    </div>
  );
}
