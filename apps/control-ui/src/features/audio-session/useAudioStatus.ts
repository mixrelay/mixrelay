import { useEffect, useState } from 'react';
import type { ControlPort, ToneNariStatus } from '../../core/control-port/types';

/** Completion-based polling prevents overlapping native requests and cancels on unmount. */
export function useAudioStatus(controlPort: ControlPort, refreshToken = 0) {
  const [status, setStatus] = useState<{
    controlPort: ControlPort;
    refreshToken: number;
    value: ToneNariStatus;
  } | null>(null);
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    setStatus(null);
    const poll = async () => {
      try {
        const next = await controlPort.getStatus();
        if (!cancelled) setStatus({ controlPort, refreshToken, value: next });
      } catch {
        // Do not leave stale green health visible after the control service disappears.
        if (!cancelled) setStatus(null);
      } finally {
        if (!cancelled) timer = setTimeout(poll, 1000);
      }
    };
    void poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [controlPort, refreshToken]);
  return status?.controlPort === controlPort && status.refreshToken === refreshToken
    ? status.value
    : null;
}
