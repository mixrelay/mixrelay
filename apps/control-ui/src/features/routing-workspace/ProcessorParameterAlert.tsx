import { useTranslation } from 'react-i18next';
import { RecoveryActions } from '../../core/ui/RecoveryActions';
import styles from './RoutingWorkspace.module.css';
import type { useProcessorParameterPublisher } from './useProcessorParameterPublisher';

/** Unapplied parameter intent is separate from graph persistence warnings. */
export function ProcessorParameterAlert({
  parameters,
  applying,
  onReload,
}: {
  parameters: ReturnType<typeof useProcessorParameterPublisher>;
  applying: boolean;
  onReload: () => void;
}) {
  const { t } = useTranslation();
  const failure = parameters.failures[0];
  if (!failure) return null;
  return (
    <div className={styles.error} role="alert" inert={parameters.busy || applying}>
      <div>{t(`routing.errors.${failure.code}`)}</div>
      <div>{t('routing.recovery.draftPreserved')}</div>
      <RecoveryActions
        retryLabel={t('routing.inventory.retry')}
        reloadLabel={t('routing.recovery.reload')}
        onRetry={parameters.retryFailed}
        onReload={onReload}
      />
    </div>
  );
}
