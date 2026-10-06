import { type ReactNode, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { VirtualEndpointSpec } from '../../core/control-port/types';
import type { DriverPort } from '../../core/driver/DriverPort';
import { ActionButton } from '../../core/ui/ActionButton';
import { ConfirmationDialog } from '../../core/ui/ConfirmationDialog';
import { DriverDialog } from '../../core/ui/DriverDialog';
import { DriverDeviceForm, DriverNativeFormats } from './DriverDeviceForm';
import styles from './DriverManagement.module.css';
import { DriverStatusCard } from './DriverStatusCard';
import { type DriverSession, useDriverSession } from './useDriverSession';

function DriverManagement({ session }: { session: DriverSession }) {
  const { t } = useTranslation();
  const [creation, setCreation] = useState<VirtualEndpointSpec | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const snapshot = session.snapshot;
  const state = snapshot?.state ?? 'unavailable';
  const settings = snapshot?.settings;
  const ready = state === 'ready';
  const template = settings?.creationTemplate;
  const remove = async () => {
    if (!removing || !confirmed) return;
    const result = await session.mutate({ action: 'remove', endpointId: removing });
    if (result) setRemoving(null);
  };
  return (
    <>
      <DriverStatusCard
        state={state}
        busy={session.busy}
        actionLabel={t('driverManagement.refresh')}
        onAction={() => void session.refresh()}
      >
        {snapshot?.installedVersion ? (
          <p className={styles.notice}>
            {t('updates.installed', { version: snapshot.installedVersion })}
          </p>
        ) : null}
        {!snapshot && session.busy ? <p role="status">{t('driverManagement.loading')}</p> : null}
        {session.error ? <p role="alert">{session.error}</p> : null}
        {!snapshot && !session.busy ? <p>{t('driverManagement.unavailable')}</p> : null}
        {settings?.devices.map((device) => (
          <section
            key={device.spec.id}
            className={styles.device}
            aria-label={device.spec.displayName}
          >
            <h3>{device.spec.displayName}</h3>
            <DriverDeviceForm
              spec={device.spec}
              settings={settings}
              busy={session.busy || !ready}
              creating={false}
              onSave={(spec) => session.mutate({ action: 'configure', spec })}
            />
            <DriverNativeFormats device={device} session={session} />
            {session.allowMultiDevice ? (
              <div className={styles.actions}>
                <ActionButton
                  size="xs"
                  color="red"
                  variant="subtle"
                  disabled={session.busy || !ready}
                  onClick={() => {
                    setConfirmed(false);
                    setRemoving(device.spec.id);
                  }}
                >
                  {t('driverManagement.remove')}
                </ActionButton>
              </div>
            ) : null}
          </section>
        ))}
        {session.allowMultiDevice && ready && settings?.canCreate && template ? (
          <div className={styles.actions}>
            <ActionButton
              size="xs"
              disabled={session.busy}
              onClick={() =>
                setCreation({ ...template, id: crypto.randomUUID(), displayName: 'MixRelay IO' })
              }
            >
              {t('driverManagement.create')}
            </ActionButton>
          </div>
        ) : null}
      </DriverStatusCard>
      <DriverDialog
        opened={creation !== null && session.allowMultiDevice}
        onClose={() => setCreation(null)}
        title={t('driverManagement.create')}
        closeLabel={t('common.cancel')}
      >
        {creation && settings ? (
          <DriverDeviceForm
            spec={creation}
            settings={settings}
            busy={session.busy}
            creating
            onSave={async (spec) => {
              const result = await session.mutate({ action: 'create', spec });
              if (result) setCreation(null);
            }}
          />
        ) : null}
        {session.error ? <p role="alert">{session.error}</p> : null}
      </DriverDialog>
      <ConfirmationDialog
        opened={removing !== null && session.allowMultiDevice}
        title={t('driverManagement.remove')}
        busy={session.busy}
        disabled={session.busy || !confirmed}
        cancelLabel={t('common.cancel')}
        onCancel={() => setRemoving(null)}
        onConfirm={() => void remove()}
      >
        <label>
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(event) => setConfirmed(event.currentTarget.checked)}
          />{' '}
          {t('driverManagement.confirmRemove')}
        </label>
        {session.error ? <p role="alert">{session.error}</p> : null}
      </ConfirmationDialog>
    </>
  );
}

export function DriverManagementAccess({
  port,
  children,
  allowMultiDevice = false,
}: {
  port: DriverPort | null;
  children?: (action: ReactNode, session: DriverSession) => ReactNode;
  allowMultiDevice?: boolean;
}) {
  const { t } = useTranslation();
  const session = useDriverSession(port, allowMultiDevice);
  const [opened, setOpened] = useState(false);
  const action = port ? (
    <ActionButton
      aria-label={t('driverManagement.title')}
      title={t('driverManagement.title')}
      aria-expanded={opened}
      aria-haspopup="dialog"
      className={styles.entry}
      size="xs"
      variant="subtle"
      onClick={() => {
        setOpened(true);
        void session.refresh();
      }}
    >
      <svg aria-hidden="true" viewBox="0 0 16 16">
        <path d="M3 3h10v10H3zM6 6v4m4-4v4M1 6h2m10 0h2M1 10h2m10 0h2" />
      </svg>
    </ActionButton>
  ) : null;
  return (
    <>
      {children?.(action, session)}
      <DriverDialog
        opened={opened}
        onClose={() => setOpened(false)}
        title={t('driverManagement.title')}
        closeLabel={t('updates.close')}
      >
        <DriverManagement session={session} />
      </DriverDialog>
    </>
  );
}
