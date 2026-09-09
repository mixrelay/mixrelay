import type { CSSProperties } from 'react';

import styles from './Meter.module.css';

interface MeterProps {
  orientation?: 'horizontal' | 'vertical';
  label: string;
  level: number;
}

export function Meter({ label, level, orientation = 'vertical' }: MeterProps) {
  const normalizedLevel = Math.max(0, Math.min(100, level));
  return (
    <span className={styles.root} data-orientation={orientation} title={`${label}: ${level}%`}>
      <span
        className={styles.fill}
        style={{ '--meter-unfilled': `${100 - normalizedLevel}%` } as CSSProperties}
      />
    </span>
  );
}
