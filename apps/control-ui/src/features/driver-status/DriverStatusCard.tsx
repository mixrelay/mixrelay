import { useTranslation } from 'react-i18next';
import type { DriverState } from '../../core/control-port/types';
import { ActionButton } from '../../core/ui/ActionButton';
import { Panel } from '../../core/ui/Panel';
import { StatusBadge, type StatusTone } from '../../core/ui/StatusBadge';

interface DriverStatusCardProps {
  state: DriverState;
  onAction?: () => void;
}

const statePresentation: Record<
  DriverState,
  { action: 'actionInstall' | 'actionRestart' | 'actionDiagnose'; label: string; tone: StatusTone }
> = {
  not_installed: { action: 'actionInstall', label: 'notInstalled', tone: 'neutral' },
  installing: { action: 'actionDiagnose', label: 'installing', tone: 'neutral' },
  restart_required: { action: 'actionRestart', label: 'restartRequired', tone: 'warning' },
  installed_not_loaded: { action: 'actionDiagnose', label: 'installedNotLoaded', tone: 'warning' },
  blocked: { action: 'actionDiagnose', label: 'blocked', tone: 'critical' },
  invalid_permissions: { action: 'actionDiagnose', label: 'invalidPermissions', tone: 'critical' },
  version_mismatch: { action: 'actionDiagnose', label: 'versionMismatch', tone: 'warning' },
  ready: { action: 'actionDiagnose', label: 'ready', tone: 'positive' },
};

export function DriverStatusCard({ onAction, state }: DriverStatusCardProps) {
  const { t } = useTranslation();
  const presentation = statePresentation[state];
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
      <p className="panel__description">{t('driver.description')}</p>
      <ActionButton disabled={isBusy} loading={isBusy} onClick={onAction}>
        {t(`driver.${presentation.action}`)}
      </ActionButton>
    </Panel>
  );
}
