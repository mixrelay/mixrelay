import { type MutableRefObject, useEffect, useRef, useState } from 'react';

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

/** A failed background check is retried with backoff (2 s, 4 s, ... up to the regular
 * check interval). A failure without a license error code means the check did not reach
 * the license service (the runtime is still starting or restarting), so it is shown
 * only once it persists. */
const CHECK_INTERVAL_MS = 60_000;
const UNREACHED_GRACE_FAILURES = 3;

function retryDelay(failures: number): number {
  return Math.min(CHECK_INTERVAL_MS, 2_000 * 2 ** (failures - 1));
}

function needsLocalReconciliation(code: LicenseErrorCode): boolean {
  return code === 'rejected' || code === 'wrong_product' || code === 'storage_unavailable';
}

interface SessionRefs {
  busy: MutableRefObject<boolean>;
  generation: MutableRefObject<number>;
  operationVersion: MutableRefObject<number>;
  failures: MutableRefObject<number>;
  schedule: MutableRefObject<(delay?: number) => void>;
}

/** Runs one port's background checks until the returned cleanup; an operation that began
 * after a check was sent makes that check's result stale. */
function startChecks(
  port: LicensingPort,
  refs: SessionRefs,
  {
    setStatus,
    setError,
  }: {
    setStatus: (status: LicensingStatus | null) => void;
    setError: (error: LicenseErrorCode | null) => void;
  },
): () => void {
  const current = ++refs.generation.current;
  let timer: ReturnType<typeof setTimeout> | undefined;
  refs.failures.current = 0;
  const schedule = (delay = CHECK_INTERVAL_MS) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(check, delay);
  };
  const check = () => {
    if (refs.busy.current) {
      schedule();
      return;
    }
    const version = refs.operationVersion.current;
    const stale = () =>
      refs.generation.current !== current || refs.operationVersion.current !== version;
    void port.getStatus().then(
      (next) => {
        if (stale()) return;
        refs.failures.current = 0;
        setStatus(next);
        setError(null);
        schedule();
      },
      (failure: unknown) => {
        if (stale()) return;
        refs.failures.current += 1;
        const code = errorCode(failure);
        if (code !== 'unknown' || refs.failures.current >= UNREACHED_GRACE_FAILURES) {
          setStatus(null);
          setError(code);
        }
        schedule(retryDelay(refs.failures.current));
      },
    );
  };
  refs.schedule.current = schedule;
  check();
  return () => {
    refs.generation.current++;
    if (timer) clearTimeout(timer);
    refs.schedule.current = () => {};
  };
}

export function useLicenseSession(port: LicensingPort | null) {
  const [status, setStatus] = useState<LicensingStatus | null>(null);
  const [error, setError] = useState<LicenseErrorCode | null>(null);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const generation = useRef(0);
  const operationVersion = useRef(0);
  const scheduleRef = useRef<(delay?: number) => void>(() => {});
  // Consecutive failed checks, background or manual; any accepted status resets it.
  const failures = useRef(0);

  useEffect(() => {
    if (!port) return;
    busyRef.current = false;
    setBusy(false);
    setStatus(null);
    setError(null);
    return startChecks(
      port,
      { busy: busyRef, generation, operationVersion, failures, schedule: scheduleRef },
      { setStatus, setError },
    );
  }, [port]);

  /** Returns whether a reconciled status was accepted. */
  async function reconcileStateError(code: LicenseErrorCode, current: number) {
    if (!port || !needsLocalReconciliation(code)) return false;
    setStatus(null);
    try {
      const latest = await port.getStatus();
      if (generation.current !== current) return false;
      failures.current = 0;
      setStatus(latest);
      return true;
    } catch (statusFailure) {
      if (generation.current === current) setError(errorCode(statusFailure));
      return false;
    }
  }

  async function handleFailure(failure: unknown, current: number) {
    if (generation.current !== current) return;
    const code = errorCode(failure);
    setError(code);
    const reconciled = await reconcileStateError(code, current);
    // The port may have changed while reconciling; its new session owns scheduling.
    if (generation.current !== current) return;
    if (reconciled) {
      scheduleRef.current();
      return;
    }
    // A failed operation may have superseded a background check; checking must continue.
    // A known status keeps the regular cadence; without one, retry with backoff.
    failures.current += 1;
    scheduleRef.current(
      status && !needsLocalReconciliation(code) ? undefined : retryDelay(failures.current),
    );
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
      if (next) {
        failures.current = 0;
        setStatus(next);
      }
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
