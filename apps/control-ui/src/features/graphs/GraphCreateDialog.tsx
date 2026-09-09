import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { ActionButton } from '../../core/ui/ActionButton';
import styles from './GraphCreateDialog.module.css';

export interface GraphCreateDialogProps {
  busy?: boolean;
  creating?: boolean;
  error?: string | null;
  onCancel: () => void;
  onCreate: (name: string) => void;
  suggestedName: string;
}

export function GraphCreateDialog({
  onCancel,
  onCreate,
  suggestedName,
  busy,
  creating = false,
  error,
}: GraphCreateDialogProps) {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [name, setName] = useState(suggestedName);
  const normalizedName = name.trim();

  useEffect(() => inputRef.current?.focus(), []);

  return (
    <div className={styles.backdrop} role="presentation">
      <section
        aria-labelledby="new-graph-title"
        aria-modal="true"
        className={styles.dialog}
        role="dialog"
      >
        <header className={styles.header}>
          <h2 id="new-graph-title">{t('studio.newGraph')}</h2>
        </header>

        <form
          onSubmit={(event) => {
            event.preventDefault();
            if (normalizedName && !busy) onCreate(normalizedName);
          }}
        >
          <label className={styles.field}>
            <span>{t('studio.graphName')}</span>
            <input
              disabled={busy}
              onChange={(event) => setName(event.currentTarget.value)}
              ref={inputRef}
              value={name}
            />
          </label>
          {error ? <p role="alert">{error}</p> : null}

          <footer className={styles.actions}>
            <ActionButton
              disabled={creating}
              color="gray"
              onClick={onCancel}
              type="button"
              variant="subtle"
            >
              {t('common.cancel')}
            </ActionButton>
            <ActionButton disabled={!normalizedName || busy} type="submit" variant="filled">
              {t('common.create')}
            </ActionButton>
          </footer>
        </form>
      </section>
    </div>
  );
}
