import type { PropsWithChildren } from 'react';
import styles from './StatusBadge.module.css';

export type StatusTone = 'neutral' | 'accent' | 'positive' | 'warning' | 'critical';

interface StatusBadgeProps extends PropsWithChildren {
  className?: string;
  tone?: StatusTone;
}

export function StatusBadge({ children, className, tone = 'neutral' }: StatusBadgeProps) {
  return (
    <span className={[styles.root, className].filter(Boolean).join(' ')} data-tone={tone}>
      {children}
    </span>
  );
}
