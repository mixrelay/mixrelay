import { useEffect, useState } from 'react';
import type { ControlPort, MixRelayStatus } from '../../core/control-port/types';

/** Completion-based polling prevents overlapping native requests and cancels on unmount. */
export function useAudioStatus(controlPort: ControlPort, refreshToken = 0) {
  const [status, setStatus] = useState<{
    controlPort: ControlPort;
    value: MixRelayStatus;
  } | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: Explicit acknowledgement refresh restarts polling without clearing the current observation.
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      try {
        const next = await controlPort.getStatus();
        if (!cancelled) setStatus({ controlPort, value: next });
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
  // Refreshing the same service retains its last observation until the reply.
  // A replacement service or a failed read must still hide the old health.
  return status?.controlPort === controlPort ? status.value : null;
}
