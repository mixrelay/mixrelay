import { useTranslation } from 'react-i18next';

import { ActionButton } from '../../core/ui/ActionButton';
import { InlineNotice } from '../../core/ui/InlineNotice';
import { RetryAlert } from '../../core/ui/RetryAlert';
import styles from './RoutingWorkspace.module.css';
import type { useProcessorCatalog } from './useProcessorCatalog';
import type { useProcessorEditors } from './useProcessorEditors';

export function ProcessorCatalogAlert({
  catalog,
}: {
  catalog: ReturnType<typeof useProcessorCatalog>;
}) {
  const { t } = useTranslation();
  if (!catalog.failure && !catalog.diagnostics.length) return null;
  const diagnostics = new Map(catalog.diagnostics.map((item) => [JSON.stringify(item), item]));
  return (
    <InlineNotice layout="banner" tone="critical">
      {t(`routing.processorControl.${catalog.failure ?? 'partial'}`)}{' '}
      <ActionButton disabled={catalog.busy} onClick={catalog.retry}>
        {t('routing.processorControl.retry')}
      </ActionButton>
      {catalog.diagnostics.length > 0 && (
        <details className={styles.scanDiagnostics}>
          <summary>{t('routing.processorControl.details', { count: diagnostics.size })}</summary>
          <ul>
            {[...diagnostics].map(([key, diagnostic]) => (
              <li key={key}>
                {diagnostic.format.toUpperCase()} ·{' '}
                {t(`routing.processorControl.${diagnostic.stage}`)}
                {diagnostic.pluginName ? ` · ${diagnostic.pluginName}` : ''}
                <p>{t(`routing.processorControl.${diagnostic.stage}Failed`)}</p>
                <details className={styles.scanTechnicalDetails}>
                  <summary>{t('routing.processorControl.technicalDetails')}</summary>
                  <pre>
                    {diagnostic.pluginId ? `${diagnostic.pluginId}\n` : ''}
                    {diagnostic.message}
                  </pre>
                </details>
              </li>
            ))}
          </ul>
        </details>
      )}
    </InlineNotice>
  );
}

export function ProcessorEditorAlerts({
  editors,
}: {
  editors: ReturnType<typeof useProcessorEditors>;
}) {
  const { t } = useTranslation();
  return [...editors.issues.values()].map((issue) => (
    <RetryAlert
      key={issue.nodeId}
      framed
      message={t('routing.processorControl.editor', { name: issue.name })}
      retryLabel={t('routing.processorControl.retry')}
      busy={issue.busy}
      onRetry={() => editors.retry(issue.nodeId)}
    />
  ));
}
