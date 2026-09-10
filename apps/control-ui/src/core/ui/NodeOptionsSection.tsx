import { UnstyledButton } from '@mantine/core';
import type { ReactNode } from 'react';
import styles from './AudioNodeCard.module.css';

/** Shared node disclosure; expanding settings never changes the cable anchor face. */
export function NodeOptionsSection({
  label,
  opened,
  onToggle,
  children,
  actions,
  disabled = false,
}: {
  label: string;
  opened: boolean;
  onToggle: () => void;
  children: ReactNode;
  actions?: ReactNode;
  disabled?: boolean;
}) {
  return (
    <>
      <NodeOptionsRow actions={actions}>
        <UnstyledButton
          aria-expanded={opened}
          className={`${styles.optionsToggle} nodrag nopan nowheel`}
          onClick={(event) => {
            event.stopPropagation();
            onToggle();
          }}
        >
          <span
            aria-hidden="true"
            className={styles.optionsChevron}
            data-opened={opened || undefined}
          >
            ›
          </span>
          <span>{label}</span>
        </UnstyledButton>
      </NodeOptionsRow>
      {opened ? (
        <div className={`${styles.optionsPanel} nodrag nopan nowheel`} inert={disabled}>
          {children}
        </div>
      ) : null}
    </>
  );
}

/** Actions stay available independently of the disclosure and its disabled settings. */
export function NodeOptionsRow({
  actions,
  children,
}: {
  actions?: ReactNode;
  children?: ReactNode;
}) {
  if (!actions && !children) return null;
  return (
    <div className={`${styles.optionsRow} nodrag nopan nowheel`}>
      {children}
      {actions ? <div className={styles.optionsActions}>{actions}</div> : null}
    </div>
  );
}
