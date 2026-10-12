import type { PropsWithChildren } from 'react';
import type { NoAppearanceOverrides } from './ActionButton';
import type { StatusTone } from './StatusBadge';
import styles from './Text.module.css';

/** Typography roles come from the theme; composition owns the surrounding spacing. */
export function Text({
  children,
  kind = 'body',
  tone = 'neutral',
  as: Element = 'p',
}: PropsWithChildren<
  NoAppearanceOverrides & {
    kind?: 'body' | 'supporting' | 'label' | 'caption';
    tone?: StatusTone | 'muted';
    as?: 'p' | 'span' | 'small';
  }
>) {
  return (
    <Element className={styles.root} data-text-role={kind} data-tone={tone}>
      {children}
    </Element>
  );
}
