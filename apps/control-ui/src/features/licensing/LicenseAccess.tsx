import { type ReactNode, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { LicensingDialog } from '../../core/licensing/LicensingFields';
import type { LicensingPort } from '../../core/licensing/LicensingPort';
import { ActionButton } from '../../core/ui/ActionButton';
import styles from './LicenseAccess.module.css';
import { LicenseDetails, LicenseTerms, licenseStateText } from './LicenseDetails';
import { StopAudioAction } from './StopAudioAction';
import { useLicenseSession } from './useLicenseSession';

export interface LicenseAccessProps {
  children: ReactNode;
  additionalActions?: ReactNode;
  onPaidAccessChange?: (paid: boolean) => void;
  onStopAudio?: () => Promise<void>;
  port: LicensingPort | null;
}

/** A desktop presentation gate. Native licensing and engine policy remain authoritative. */
export function LicenseAccess({ children, additionalActions, onStopAudio, onPaidAccessChange, port }: LicenseAccessProps) {
  const { t } = useTranslation();
  const [dialogOpen, setDialogOpen] = useState(false);
  const { status, error, busy, run, clearError } = useLicenseSession(port);
  const reportedPort = useRef(port);
  useEffect(() => {
    if (reportedPort.current !== port) {
      reportedPort.current = port;
      onPaidAccessChange?.(false);
      return;
    }
    onPaidAccessChange?.(Boolean(port && status?.status === 'licensed'));
  }, [onPaidAccessChange, port, status?.status]);

  if (!port) return <>{children}</>;

  if (!status) {
    return (
      <main className={styles.blocked}>
        <section aria-labelledby="licensing-title" className={styles.card}>
          <h1 id="licensing-title">{t('licensing.title')}</h1>
          {error ? (
            <>
              <p role="alert">{t(`licensing.errors.${error}`)}</p>
              <ActionButton disabled={busy} onClick={() => void run(() => port.getStatus())}>
                {t('licensing.retry')}
              </ActionButton>
            </>
          ) : (
            <p role="status">{t('licensing.loading')}</p>
          )}
          {error && onStopAudio ? <StopAudioAction onStopAudio={onStopAudio} /> : null}
          {additionalActions}
          <LicenseTerms />
        </section>
      </main>
    );
  }

  const details = (
    <LicenseDetails
      busy={busy}
      error={error}
      onMutated={() => setDialogOpen(false)}
      port={port}
      run={run}
      status={status}
    />
  );
  const active = status.status === 'trial' || status.status === 'licensed';
  if (!active) {
    return (
      <main className={styles.blocked}>
        <section aria-labelledby="licensing-title" className={styles.card}>
          <h1 id="licensing-title">{t('licensing.title')}</h1>
          <p>{t(`licensing.messages.${status.status}`)}</p>
          {details}
          {onStopAudio ? <StopAudioAction onStopAudio={onStopAudio} /> : null}
          {additionalActions}
        </section>
      </main>
    );
  }

  return (
    <>
      {children}
      <aside className={styles.entry} aria-label={t('licensing.title')}>
        <ActionButton
          aria-expanded={dialogOpen}
          aria-haspopup="dialog"
          onClick={() => {
            clearError();
            setDialogOpen(true);
          }}
          size="xs"
          variant="light"
        >
          {licenseStateText(status, t)}
        </ActionButton>
        {error && !dialogOpen ? (
          <p className={styles.entryError} role="alert">
            {t(`licensing.errors.${error}`)}
          </p>
        ) : null}
      </aside>
      <LicensingDialog
        busy={busy}
        closeLabel={t('licensing.close')}
        onClose={() => setDialogOpen(false)}
        opened={dialogOpen}
        title={t('licensing.title')}
      >
        {details}
      </LicensingDialog>
    </>
  );
}
