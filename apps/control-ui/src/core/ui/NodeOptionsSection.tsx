import { UnstyledButton } from '@mantine/core';
import type { ReactNode } from 'react';
import styles from './AudioNodeCard.module.css';

/** Shared node disclosure; expanding settings never changes the cable anchor face. */
export function NodeOptionsSection({
  label,
  opened,
  onToggle,
  children,
}: {
  label: string;
  opened: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <>
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
      {opened ? (
        <div className={`${styles.optionsPanel} nodrag nopan nowheel`}>{children}</div>
      ) : null}
    </>
  );
}
