import { type ReactNode, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAppInformation } from '../../core/app-info/useAppInformation';
import { LicensingDialog } from '../../core/licensing/LicensingFields';
import type { LicensingPort } from '../../core/ports/LicensingPort';
import { ActionButton } from '../../core/ui/ActionButton';
import { AppInformationButton } from '../../core/ui/AppInformationButton';
import { InlineNotice } from '../../core/ui/InlineNotice';
import { AboutDialog } from './AboutDialog';
import styles from './LicenseAccess.module.css';
import { LicenseDetails, licenseStateText } from './LicenseDetails';
import { useLicenseSession } from './useLicenseSession';

export interface LicenseAccessProps {
  children: ReactNode | ((action: ReactNode) => ReactNode);
  presentation?: 'license' | 'footer';
  onPaidAccessChange?: (paid: boolean) => void;
  port: LicensingPort | null;
}

/** A desktop presentation gate. Native licensing and engine policy remain authoritative. */
export function LicenseAccess({
  children,
  onPaidAccessChange,
  port,
  presentation = 'license',
}: LicenseAccessProps) {
  const { t } = useTranslation();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const { status, error, busy, run, clearError } = useLicenseSession(port);
  const footer = presentation === 'footer';
  const openDialog = () => {
    clearError();
    setAboutOpen(false);
    setDialogOpen(true);
  };
  const version = useAppInformation(() => {
    if (busy) return;
    setDialogOpen(false);
    setAboutOpen(true);
  }, footer);
  const reportedPort = useRef(port);
  useEffect(() => {
    if (reportedPort.current !== port) {
      reportedPort.current = port;
      onPaidAccessChange?.(false);
      return;
    }
    onPaidAccessChange?.(Boolean(port && status?.status === 'licensed'));
  }, [onPaidAccessChange, port, status?.status]);

  if (!port && !footer) return <>{typeof children === 'function' ? children(null) : children}</>;
  const title = t('licensing.title');
  const action = (
    <LicenseEntry
      footer={footer}
      version={version}
      status={status}
      error={error}
      opened={dialogOpen}
      onOpen={openDialog}
    />
  );
  return (
    <>
      {typeof children === 'function' ? children(action) : children}
      {typeof children !== 'function' ? (
        <aside className={styles.entry} aria-label={title}>
          {action}
        </aside>
      ) : null}
      <LicensingDialog
        busy={busy}
        closeLabel={t('licensing.close')}
        onClose={() => setDialogOpen(false)}
        opened={dialogOpen}
        title={title}
      >
        <LicenseContents
          session={{ status, error, busy, run }}
          port={port}
          onMutated={() => setDialogOpen(false)}
        />
      </LicensingDialog>
      {footer ? (
        <AboutDialog version={version} opened={aboutOpen} onClose={() => setAboutOpen(false)} />
      ) : null}
    </>
  );
}

type Session = ReturnType<typeof useLicenseSession>;
function LicenseEntry({
  footer,
  version,
  status,
  error,
  opened,
  onOpen,
}: {
  footer: boolean;
  version: string;
  status: Session['status'];
  error: Session['error'];
  opened: boolean;
  onOpen: () => void;
}) {
  const { t } = useTranslation();
  const state = footer
    ? status?.status === 'licensed'
      ? t('about.licensed')
      : status
        ? licenseStateText(status, t)
        : t('about.unknown')
    : status
      ? licenseStateText(status, t)
      : t('licensing.states.free');
  return (
    <div className={styles.infoEntry}>
      {footer ? (
        <AppInformationButton
          label={t('licensing.title')}
          version={version}
          status={state}
          opened={opened}
          onOpen={onOpen}
        />
      ) : (
        <ActionButton
          aria-expanded={opened}
          aria-haspopup="dialog"
          onClick={onOpen}
          density="compact"
        >
          {state}
        </ActionButton>
      )}
      {error && !opened ? (
        <div className={styles.entryError}>
          <InlineNotice tone="critical">{t(`licensing.errors.${error}`)}</InlineNotice>
        </div>
      ) : null}
    </div>
  );
}

function LicenseContents({
  session,
  port,
  onMutated,
}: {
  session: Pick<Session, 'status' | 'error' | 'busy' | 'run'>;
  port: LicensingPort | null;
  onMutated: () => void;
}) {
  const { t } = useTranslation();
  const { status, error, busy, run } = session;
  if (!port) return <p>{t('about.unknown')}</p>;
  if (status)
    return (
      <LicenseDetails
        busy={busy}
        error={error}
        onMutated={onMutated}
        port={port}
        run={run}
        status={status}
      />
    );
  return (
    <>
      {error ? (
        <p role="alert">{t(`licensing.errors.${error}`)}</p>
      ) : (
        <p role="status">{t('licensing.loading')}</p>
      )}
      <ActionButton disabled={busy} onClick={() => void run(() => port.getStatus())}>
        {t('licensing.retry')}
      </ActionButton>
    </>
  );
}
