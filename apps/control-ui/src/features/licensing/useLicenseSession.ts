import { useEffect, useRef, useState } from 'react';

import type { LicensingPort, LicensingStatus } from '../../core/licensing/LicensingPort';

const errorCodes = [
  'invalid_configuration',
  'invalid_input',
  'wrong_product',
  'rejected',
  'unavailable',
  'rate_limited',
  'invalid_response',
  'storage_unavailable',
  'invalid_state',
  'clock_invalid',
  'request_failed',
] as const;

export type LicenseErrorCode = (typeof errorCodes)[number] | 'unknown';

function errorCode(error: unknown): LicenseErrorCode {
  const message = typeof error === 'string' ? error : error instanceof Error ? error.message : null;
  return errorCodes.find((code) => code === message) ?? 'unknown';
}

function needsLocalReconciliation(code: LicenseErrorCode): boolean {
  return code === 'rejected' || code === 'wrong_product' || code === 'storage_unavailable';
}

export function useLicenseSession(port: LicensingPort | null) {
  const [status, setStatus] = useState<LicensingStatus | null>(null);
  const [error, setError] = useState<LicenseErrorCode | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const generation = useRef(0);
  const operationVersion = useRef(0);
  const scheduleRef = useRef<() => void>(() => {});

  useEffect(() => {
    if (!port) return;
    const current = ++generation.current;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const schedule = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(check, 60_000);
    };
    const check = () => {
      if (busyRef.current) {
        schedule();
        return;
      }
      const version = operationVersion.current;
      void port.getStatus().then(
        (next) => {
          if (generation.current !== current || operationVersion.current !== version) return;
          setStatus(next);
          setError(null);
          schedule();
        },
        (failure: unknown) => {
          if (generation.current !== current || operationVersion.current !== version) return;
          setStatus(null);
          setError(errorCode(failure));
        },
      );
    };
    scheduleRef.current = schedule;
    busyRef.current = false;
    setBusy(false);
    setStatus(null);
    setError(null);
    check();
    return () => {
      generation.current++;
      if (timer) clearTimeout(timer);
      scheduleRef.current = () => {};
    };
  }, [port]);

  async function reconcileStateError(code: LicenseErrorCode, current: number) {
    if (!port || !needsLocalReconciliation(code)) return;
    setStatus(null);
    try {
      const latest = await port.getStatus();
      if (generation.current === current) {
        setStatus(latest);
        scheduleRef.current();
      }
    } catch (statusFailure) {
      if (generation.current === current) setError(errorCode(statusFailure));
    }
  }

  async function handleFailure(failure: unknown, current: number) {
    if (generation.current !== current) return;
    const code = errorCode(failure);
    setError(code);
    await reconcileStateError(code, current);
    if (status && !needsLocalReconciliation(code)) scheduleRef.current();
  }

  async function run(action: () => Promise<LicensingStatus> | Promise<void>): Promise<boolean> {
    if (busyRef.current) return false;
    busyRef.current = true;
    operationVersion.current++;
    const current = generation.current;
    setBusy(true);
    setError(null);
    try {
      const next = await action();
      if (generation.current !== current) return false;
      if (next) setStatus(next);
      scheduleRef.current();
      return true;
    } catch (failure) {
      await handleFailure(failure, current);
      return false;
    } finally {
      if (generation.current === current) {
        busyRef.current = false;
        setBusy(false);
      }
    }
  }

  return { status, error, busy, run, clearError: () => setError(null) };
}
