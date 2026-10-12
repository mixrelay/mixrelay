import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { RuntimePort, RuntimeStatus } from '../../core/runtime/RuntimePort';
import { ActionButton } from '../../core/ui/ActionButton';
import { BrandIdentity } from '../../core/ui/BrandIdentity';
import styles from './RuntimeConnection.module.css';

import { useRuntimeConnection } from './useRuntimeConnection';

/** Mount the workspace only after Driver and engine readiness; preserve it during disconnection. */
export function RuntimeConnection({
  port,
  onInstall,
  children,
}: {
  port: RuntimePort;
  onInstall?: () => void;
  children: ReactNode;
}) {
  const { state, opened, connect } = useRuntimeConnection(port);

  return (
    <>
      {opened ? (
        <div hidden={state !== 'ready'} inert={state !== 'ready'}>
          {children}
        </div>
      ) : null}
      {state !== 'ready' ? (
        <ConnectionScreen state={state} onInstall={onInstall} onRetry={connect} />
      ) : null}
    </>
  );
}

function ConnectionScreen({
  state,
  onInstall,
  onRetry,
}: {
  state: Exclude<RuntimeStatus, 'ready'> | 'connecting';
  onInstall?: () => void;
  onRetry: () => Promise<void>;
}) {
  const { t } = useTranslation();
  if (state === 'connecting')
    return (
      <main className={styles.screen} aria-label="MixRelay" aria-busy="true">
        <span className={styles.spinner} aria-hidden="true" />
        <p className={styles.statusText} role="status">
          {t('runtimeConnection.connecting.title')}
        </p>
      </main>
    );
  return (
    <main className={styles.screen}>
      <section className={styles.content} aria-labelledby="runtime-title">
        <p className={styles.brand}>
          <BrandIdentity size="standard" />
        </p>
        <h1 id="runtime-title">{t(`runtimeConnection.${state}.title`)}</h1>
        <p role="alert">{t(`runtimeConnection.${state}.description`)}</p>
        <div className={styles.actions}>
          {onInstall && (state === 'not_installed' || state === 'incompatible') ? (
            <ActionButton onClick={onInstall}>
              {t(
                state === 'incompatible' ? 'runtimeConnection.update' : 'runtimeConnection.install',
              )}
            </ActionButton>
          ) : null}
          <ActionButton onClick={() => void onRetry()}>{t('runtimeConnection.retry')}</ActionButton>
        </div>
      </section>
    </main>
  );
}
