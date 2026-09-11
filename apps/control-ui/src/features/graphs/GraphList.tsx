import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { GraphCatalog } from '../../core/control-port/types';
import { ConfirmationDialog } from '../../core/ui/ConfirmationDialog';
import styles from './GraphList.module.css';

interface GraphListProps {
  catalog: GraphCatalog;
  disabled: boolean;
  busy?: boolean;
  error: string | null;
  onSelect: (id: string) => void;
  onDelete: (id: string) => void;
}

export function GraphList({
  catalog,
  disabled,
  busy = false,
  error,
  onSelect,
  onDelete,
}: GraphListProps) {
  const { t } = useTranslation();
  const [target, setTarget] = useState<string | null>(null);
  const graph = catalog.graphs.find((entry) => entry.id === target);
  useEffect(() => {
    if (!graph) setTarget(null);
  }, [graph]);
  return (
    <>
      {catalog.graphs.map((entry) => (
        <fieldset className={styles.row} key={entry.id} aria-label={entry.name}>
          <button
            type="button"
            className={styles.select}
            aria-current={entry.id === catalog.activeId ? 'page' : undefined}
            disabled={disabled || entry.id === catalog.activeId}
            onClick={() => onSelect(entry.id)}
          >
            {entry.name}
          </button>
          <button
            type="button"
            className={styles.remove}
            aria-label={t('studio.deleteGraph')}
            disabled={disabled}
            onClick={() => setTarget(entry.id)}
          >
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M3 4h10M6 4V2h4v2M4 4l.6 10h6.8L12 4M6.5 6.5v5M9.5 6.5v5" />
            </svg>
          </button>
        </fieldset>
      ))}
      <ConfirmationDialog
        opened={Boolean(graph)}
        onCancel={() => {
          if (!busy) setTarget(null);
        }}
        title={t('studio.deleteGraph')}
        cancelLabel={t('common.cancel')}
        busy={busy}
        disabled={disabled || !graph}
        onConfirm={() => {
          if (graph) onDelete(graph.id);
        }}
      >
        <p className={styles.notice}>{t('studio.deleteGraphNotice', { name: graph?.name })}</p>
        {error ? <p role="alert">{error}</p> : null}
      </ConfirmationDialog>
    </>
  );
}
