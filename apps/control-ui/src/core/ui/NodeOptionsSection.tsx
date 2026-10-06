import { UnstyledButton } from '@mantine/core';
import { useResizeObserver } from '@mantine/hooks';
import { createContext, type ReactNode, useContext, useLayoutEffect } from 'react';
import styles from './AudioNodeCard.module.css';

export const NodeDisclosureMeasureContext = createContext<(() => void) | null>(null);

/** Shared node disclosure; expanding settings never changes the cable anchor face. */
export function NodeOptionsSection({
  label,
  opened,
  onToggle,
  children,
  disabled = false,
}: {
  label: string;
  opened: boolean;
  onToggle: () => void;
  children: ReactNode;
  disabled?: boolean;
}) {
  const measureDisclosure = useContext(NodeDisclosureMeasureContext);
  const [panelRef, panelBounds] = useResizeObserver<HTMLDivElement>();
  useLayoutEffect(() => {
    // The panel's observer and open state only signal when to read both live boxes.
    void opened;
    void panelBounds.height;
    void panelBounds.width;
    measureDisclosure?.();
  }, [opened, panelBounds.height, panelBounds.width, measureDisclosure]);
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
        <div
          ref={panelRef}
          data-node-disclosure=""
          className={`${styles.optionsPanel} nodrag nopan nowheel`}
          inert={disabled}
        >
          {children}
        </div>
      ) : null}
    </>
  );
}
