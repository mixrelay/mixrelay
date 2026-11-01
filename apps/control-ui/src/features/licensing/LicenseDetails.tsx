import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { LicensingStatus } from '../../core/domain/licensing';
import { formattingLocale } from '../../core/i18n';
import { LicensingKeyField } from '../../core/licensing/LicensingFields';
import type { LicensingPort } from '../../core/ports/LicensingPort';
import { ActionButton } from '../../core/ui/ActionButton';
import { FormActions } from '../../core/ui/FormActions';
import { Stack } from '../../core/ui/Stack';
import { TextField } from '../../core/ui/TextField';
import styles from './LicenseAccess.module.css';
import type { LicenseErrorCode } from './useLicenseSession';

export function licenseStateText(
  status: LicensingStatus,
  t: ReturnType<typeof useTranslation>['t'],
) {
  return t(`licensing.states.${status.status}`, { count: status.remainingDays });
}

function expiryDate(unix: number | null, locale: string): string | null {
  if (unix == null || !Number.isFinite(unix)) return null;
  const date = new Date(unix * 1000);
  if (Number.isNaN(date.getTime())) return null;
  return new Intl.DateTimeFormat(formattingLocale(locale), {
    dateStyle: 'medium',
  }).format(date);
}

function ExpiryLine({ status }: { status: LicensingStatus }) {
  const { i18n, t } = useTranslation();
  if (status.status !== 'licensed') return null;
  const expires = expiryDate(status.expiresAtUnix, i18n.language);
  if (!expires) return null;
  const label = 'licensing.expires';
  return <p>{t(label, { date: expires })}</p>;
}

function hasStateLine(status: LicensingStatus): boolean {
  return status.status === 'licensed';
}

export interface LicenseDetailsProps {
  busy: boolean;
  error: LicenseErrorCode | null;
  onMutated: () => void;
  port: LicensingPort;
  run: (action: () => Promise<LicensingStatus> | Promise<void>) => Promise<boolean>;
  status: LicensingStatus;
}

function ActivationForm({
  busy,
  onMutated,
  port,
  run,
}: Omit<LicenseDetailsProps, 'error' | 'status'>) {
  const { t } = useTranslation();
  const [key, setKey] = useState('');
  const [instanceName, setInstanceName] = useState('');

  async function activate() {
    if (!key.trim() || !instanceName.trim() || busy) return;
    if (await run(() => port.activate(key.trim(), instanceName.trim()))) {
      setKey('');
      setInstanceName('');
      onMutated();
    }
  }

  return (
    <form
      className={styles.form}
      onSubmit={(event) => {
        event.preventDefault();
        void activate();
      }}
    >
      <Stack>
        <LicensingKeyField
          disabled={busy}
          label={t('licensing.key')}
          onChange={setKey}
          value={key}
        />
        <TextField
          autoComplete="off"
          disabled={busy}
          label={t('licensing.instanceName')}
          onChange={(event) => setInstanceName(event.currentTarget.value)}
          value={instanceName}
        />
      </Stack>
      <FormActions>
        <ActionButton
          disabled={busy || !key.trim() || !instanceName.trim()}
          type="submit"
          emphasis="strong"
        >
          {t('licensing.activate')}
        </ActionButton>
      </FormActions>
    </form>
  );
}

export function LicenseDetails({ busy, error, onMutated, port, run, status }: LicenseDetailsProps) {
  const { t } = useTranslation();
  const hasActivation = status.hasActivation ?? status.status === 'licensed';
  const canActivate = status.status === 'free' && !hasActivation;

  return (
    <div className={styles.details}>
      {hasStateLine(status) ? <p className={styles.state}>{licenseStateText(status, t)}</p> : null}
      <ExpiryLine status={status} />
      {status.status !== 'licensed' ? <p>{t('licensing.freeSummary')}</p> : null}
      {!status.configured ? <p>{t('licensing.storeUnavailable')}</p> : null}
      {error ? <p role="alert">{t(`licensing.errors.${error}`)}</p> : null}
      {status.status === 'clock_invalid' ? (
        <FormActions>
          <ActionButton disabled={busy} onClick={() => void run(() => port.getStatus())}>
            {t('licensing.retry')}
          </ActionButton>
        </FormActions>
      ) : null}
      {status.configured && canActivate ? (
        <ActivationForm busy={busy} onMutated={onMutated} port={port} run={run} />
      ) : null}
      <LicenseActions busy={busy} onMutated={onMutated} port={port} run={run} status={status} />
    </div>
  );
}

function LicenseActions({
  busy,
  onMutated,
  port,
  run,
  status,
}: Omit<LicenseDetailsProps, 'error'>) {
  const { t } = useTranslation();
  const hasActivation = status.hasActivation ?? status.status === 'licensed';
  const canActivate = status.status === 'free' && !hasActivation;
  if (!status.configured || status.status === 'clock_invalid') return null;
  return (
    <FormActions>
      {canActivate ? (
        <ActionButton disabled={busy} onClick={() => void run(() => port.openCheckout())}>
          {t('licensing.purchase')}
        </ActionButton>
      ) : null}
      {hasActivation ? (
        <ActionButton disabled={busy} onClick={() => void run(() => port.validate())}>
          {t('licensing.validate')}
        </ActionButton>
      ) : null}
      {hasActivation ? (
        <ActionButton
          disabled={busy}
          onClick={() => {
            void run(() => port.deactivate()).then((success) => {
              if (success) onMutated();
            });
          }}
        >
          {t('licensing.deactivate')}
        </ActionButton>
      ) : null}
    </FormActions>
  );
}
