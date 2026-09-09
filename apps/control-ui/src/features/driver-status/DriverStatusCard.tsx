import { useTranslation } from 'react-i18next';
import type { DriverState } from '../../core/control-port/types';
import { ActionButton } from '../../core/ui/ActionButton';
import { Panel } from '../../core/ui/Panel';
import { StatusBadge } from '../../core/ui/StatusBadge';
import styles from './DriverStatusCard.module.css';
import { getDriverPresentation } from './presentation';

interface DriverStatusCardProps {
  state: DriverState;
  onAction?: () => void;
}

export function DriverStatusCard({ onAction, state }: DriverStatusCardProps) {
  const { t } = useTranslation();
  const presentation = getDriverPresentation(state);
  const isBusy = state === 'installing';

  return (
    <Panel
      title={t('driver.title')}
      trailing={
        <StatusBadge tone={presentation.tone}>
          {t(`driver.states.${presentation.label}`)}
        </StatusBadge>
      }
    >
      <p className={styles.description}>{t('driver.description')}</p>
      <ActionButton disabled={isBusy} loading={isBusy} onClick={onAction}>
        {t(`driver.${presentation.action}`)}
      </ActionButton>
    </Panel>
  );
}
