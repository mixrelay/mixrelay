import { useTranslation } from 'react-i18next';

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
    <div className={styles.error}>
      <RetryAlert
        message={t(`routing.processorControl.${catalog.failure ?? 'partial'}`)}
        retryLabel={t('routing.processorControl.retry')}
        busy={catalog.busy}
        onRetry={catalog.retry}
      />
      {catalog.diagnostics.length > 0 && (
        <details className={styles.scanDiagnostics}>
          <summary>
            {t('routing.processorControl.details', { count: catalog.diagnostics.length })}
          </summary>
          <ul>
            {[...diagnostics].map(([key, diagnostic]) => (
              <li key={key}>
                {diagnostic.format.toUpperCase()} ·{' '}
                {t(`routing.processorControl.${diagnostic.stage}`)}
                {diagnostic.pluginId ? ` · ${diagnostic.pluginId}` : ''}: {diagnostic.message}
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
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
      className={styles.error}
      message={t('routing.processorControl.editor', { name: issue.name })}
      retryLabel={t('routing.processorControl.retry')}
      busy={issue.busy}
      onRetry={() => editors.retry(issue.nodeId)}
    />
  ));
}
