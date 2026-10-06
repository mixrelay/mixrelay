import { useTranslation } from 'react-i18next';
import type { ReactNode } from 'react';
import type { DriverState } from '../../core/control-port/types';
import { ActionButton } from '../../core/ui/ActionButton';
import { Panel } from '../../core/ui/Panel';
import { StatusBadge } from '../../core/ui/StatusBadge';
import styles from './DriverStatusCard.module.css';
import { getDriverPresentation } from './presentation';

interface DriverStatusCardProps {
  state: DriverState;
  onAction?: () => void;
  actionLabel?: string;
  children?: ReactNode;
  busy?: boolean;
}

export function DriverStatusCard({ onAction, state, actionLabel, children, busy }: DriverStatusCardProps) {
  const { t } = useTranslation();
  const presentation = getDriverPresentation(state);
  const isBusy = busy || state === 'installing';

  return (
    <Panel
      title={t('driver.title')}
      trailing={
        <StatusBadge tone={presentation.tone}>
          {t(`driver.states.${presentation.label}`)}
        </StatusBadge>
      }
    >
      {children ?? <p className={styles.description}>{t('driver.description')}</p>}
      {onAction ? <ActionButton size="xs" disabled={isBusy} loading={isBusy} onClick={onAction}>
        {actionLabel ?? t(`driver.${presentation.action}`)}
      </ActionButton> : null}
    </Panel>
  );
}
