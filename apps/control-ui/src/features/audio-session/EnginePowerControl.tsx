import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  ControlPort,
  MixRelayStatus,
  RuntimeGraphDocument,
} from '../../core/control-port/types';
import styles from './EnginePowerControl.module.css';
import { getEnginePresentation } from './presentation';
import { waitForEnginePower } from './waitForEnginePower';
import { waitForGraphActivation } from './waitForGraphActivation';

export function EnginePowerControl({
  controlPort,
  status,
  disabled,
  onBusyChange,
  onApplied,
}: {
  controlPort: ControlPort;
  status: MixRelayStatus | null;
  disabled?: boolean;
  onBusyChange: (busy: boolean) => void;
  onApplied: (document: RuntimeGraphDocument) => void;
}) {
  const { t } = useTranslation();
  const lock = useRef(false);
  const generation = useRef(0);
  const owner = useRef(controlPort);
  owner.current = controlPort;
  useEffect(
    () => () => {
      generation.current += 1;
    },
    [],
  );
  const [confirmed, setConfirmed] = useState<{
    base: MixRelayStatus | null;
    value: MixRelayStatus;
  }>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const power = (confirmed?.base === status ? confirmed.value : status)?.enginePower;
  const pending = busy || power?.transitioning;
  const label = !power
    ? getEnginePresentation(status, t).label
    : pending
      ? t('audioSession.powerChanging')
      : power?.enabled
        ? t('audioSession.powerOff')
        : t('audioSession.powerOn');
  const toggle = async () => {
    if (lock.current || !power || !controlPort.setEnginePower) return;
    lock.current = true;
    setBusy(true);
    setError(false);
    onBusyChange(true);
    const ticket = generation.current;
    const current = () => ticket === generation.current && owner.current === controlPort;
    try {
      const response = await controlPort.setEnginePower(!power.enabled);
      if (!(await waitForGraphActivation(controlPort, response.revision, current))) return;
      const document = await controlPort.getGraph();
      const next = await waitForEnginePower(controlPort, !power.enabled, current);
      if (!next) return;
      setConfirmed({ base: status, value: next });
      onApplied(document);
    } catch {
      if (current()) setError(true);
    } finally {
      lock.current = false;
      setBusy(false);
      onBusyChange(false);
    }
  };
  return (
    <div className={styles.control}>
      {error ? <span role="alert">{t('audioSession.powerFailed')}</span> : null}
      <button
        type="button"
        className={styles.button}
        aria-label={label}
        title={label}
        aria-pressed={Boolean(power?.enabled)}
        aria-busy={Boolean(pending)}
        disabled={disabled || pending || !power || !controlPort.setEnginePower}
        onClick={() => void toggle()}
      >
        <svg viewBox="0 0 16 16" aria-hidden="true">
          <path d="M8 1.5v6M4.2 3.6a5.5 5.5 0 1 0 7.6 0" />
        </svg>
        <span>{t('studio.engineShort')}</span>
      </button>
    </div>
  );
}
