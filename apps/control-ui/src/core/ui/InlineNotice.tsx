import type { FocusEventHandler, ReactNode } from 'react';
import type { NoAppearanceOverrides } from './ActionButton';
import styles from './InlineNotice.module.css';
import type { StatusTone } from './StatusBadge';

/** Local feedback stays with the affected operation; critical changes are announced urgently. */
export function InlineNotice({
  children,
  title,
  tone = 'neutral',
  actions,
  layout = 'inline',
  announcement,
  inert,
  onFocusCapture,
  onBlurCapture,
}: NoAppearanceOverrides & {
  children: ReactNode;
  title?: string;
  tone?: StatusTone;
  actions?: ReactNode;
  layout?: 'inline' | 'banner';
  announcement?: 'status' | 'alert';
  inert?: boolean;
  onFocusCapture?: FocusEventHandler<HTMLDivElement>;
  onBlurCapture?: FocusEventHandler<HTMLDivElement>;
}) {
  return (
    <div
      className={styles.root}
      data-tone={tone}
      data-layout={layout}
      role={announcement ?? (tone === 'critical' ? 'alert' : 'status')}
      inert={inert}
      onFocusCapture={onFocusCapture}
      onBlurCapture={onBlurCapture}
    >
      <div className={styles.content}>
        {title ? <strong className={styles.title}>{title}</strong> : null}
        <div>{children}</div>
      </div>
      {actions ? <div className={styles.actions}>{actions}</div> : null}
    </div>
  );
}
