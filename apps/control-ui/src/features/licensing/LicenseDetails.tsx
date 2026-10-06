import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import licenseTerms from '../../../../../LICENSE.txt?raw';
import { LicensingKeyField } from '../../core/licensing/LicensingFields';
import type { LicensingPort, LicensingStatus } from '../../core/licensing/LicensingPort';
import { ActionButton } from '../../core/ui/ActionButton';
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
  return new Intl.DateTimeFormat(locale === 'ko-KR' ? 'ko-KR' : 'en', {
    dateStyle: 'medium',
  }).format(date);
}

function ExpiryLine({ status }: { status: LicensingStatus }) {
  const { i18n, t } = useTranslation();
  if (status.status === 'clock_invalid') return null;
  const expires = expiryDate(status.expiresAtUnix, i18n.language);
  if (!expires) return null;
  const label = status.status === 'expired' ? 'licensing.expiredOn' : 'licensing.expires';
  return <p>{t(label, { date: expires })}</p>;
}

function hasStateLine(status: LicensingStatus): boolean {
  return status.status === 'trial' || status.status === 'licensed';
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
      <LicensingKeyField disabled={busy} label={t('licensing.key')} onChange={setKey} value={key} />
      <TextField
        autoComplete="off"
        disabled={busy}
        label={t('licensing.instanceName')}
        onChange={(event) => setInstanceName(event.currentTarget.value)}
        value={instanceName}
      />
      <ActionButton disabled={busy || !key.trim() || !instanceName.trim()} type="submit">
        {t('licensing.activate')}
      </ActionButton>
    </form>
  );
}

export function LicenseTerms() {
  const { t } = useTranslation();

  return (
    <div className={styles.terms}>
      <p>{t('licensing.integrationNotice')}</p>
      <p>
        <a href="mailto:support@mixrelay.com">support@mixrelay.com</a>
      </p>
      <details>
        <summary>{t('licensing.termsTitle')}</summary>
        <pre className={styles.termsText}>{licenseTerms}</pre>
      </details>
    </div>
  );
}

export function LicenseDetails({ busy, error, onMutated, port, run, status }: LicenseDetailsProps) {
  const { t } = useTranslation();
  const canActivate = status.status === 'trial' || status.status === 'expired';

  return (
    <div className={styles.details}>
      {hasStateLine(status) ? <p className={styles.state}>{licenseStateText(status, t)}</p> : null}
      <ExpiryLine status={status} />
      {!status.configured ? <p>{t('licensing.storeUnavailable')}</p> : null}
      {error ? <p role="alert">{t(`licensing.errors.${error}`)}</p> : null}
      {status.status === 'clock_invalid' ? (
        <div className={styles.actions}>
          <ActionButton disabled={busy} onClick={() => void run(() => port.getStatus())}>
            {t('licensing.retry')}
          </ActionButton>
        </div>
      ) : null}
      {status.configured && canActivate ? (
        <ActivationForm busy={busy} onMutated={onMutated} port={port} run={run} />
      ) : null}
      {status.configured && status.status !== 'clock_invalid' ? (
        <div className={styles.actions}>
          {canActivate ? (
            <ActionButton disabled={busy} onClick={() => void run(() => port.openCheckout())}>
              {t('licensing.purchase')}
            </ActionButton>
          ) : null}
          {status.status !== 'trial' ? (
            <ActionButton disabled={busy} onClick={() => void run(() => port.validate())}>
              {t('licensing.validate')}
            </ActionButton>
          ) : null}
          {status.status === 'licensed' ? (
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
        </div>
      ) : null}
      <LicenseTerms />
    </div>
  );
}
