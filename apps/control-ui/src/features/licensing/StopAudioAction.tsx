import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { ActionButton } from '../../core/ui/ActionButton';
import styles from './LicenseAccess.module.css';

export function StopAudioAction({ onStopAudio }: { onStopAudio: () => Promise<void> }) {
  const { t } = useTranslation();
  const [state, setState] = useState<'idle' | 'busy' | 'sent' | 'failed'>('idle');
  const pending = useRef(false);

  async function stop() {
    if (pending.current) return;
    pending.current = true;
    setState('busy');
    try {
      await onStopAudio();
      setState('sent');
    } catch {
      setState('failed');
    } finally {
      pending.current = false;
    }
  }

  return (
    <div className={styles.stopAudio}>
      <ActionButton disabled={state === 'busy' || state === 'sent'} onClick={() => void stop()}>
        {t('licensing.stopAudio')}
      </ActionButton>
      {state === 'sent' ? <p role="status">{t('licensing.stopRequested')}</p> : null}
      {state === 'failed' ? <p role="alert">{t('licensing.stopFailed')}</p> : null}
    </div>
  );
}
