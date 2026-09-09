import type { PropsWithChildren } from 'react';
import styles from './WorkspaceHeader.module.css';

/** One sizing rule for adjacent workspace headers, including their bottom border. */
export function WorkspaceHeader({
  children,
  className,
}: PropsWithChildren<{ className?: string }>) {
  return <header className={[styles.root, className].filter(Boolean).join(' ')}>{children}</header>;
}
