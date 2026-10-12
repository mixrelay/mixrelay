import { useEffect, useState } from 'react';
import type { ControlPort, MixRelayStatus } from '../../core/control-port/types';
import { structuralShare } from '../../core/structuralShare';

type Observation = { controlPort: ControlPort; value: MixRelayStatus } | null;

/** The first reply after a (re)start is always new, even when a port hands back the same
 * object; later replies keep the previous identity when nothing changed. */
function observation(controlPort: ControlPort, value: MixRelayStatus, first: boolean) {
  return (previous: Observation): Observation =>
    first
      ? { controlPort, value: { ...value } }
      : structuralShare(previous, { controlPort, value });
}

/** Completion-based polling prevents overlapping native requests and cancels on unmount. */
export function useAudioStatus(controlPort: ControlPort, refreshToken = 0) {
  // An unchanged reply keeps the previous object, so a quiet poll renders nothing. The
  // first reply after a (re)start is always a new object: an acknowledgement refresh
  // marks a new observation that confirmations compare against by identity.
  const [status, setStatus] = useState<Observation>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: Explicit acknowledgement refresh restarts polling without clearing the current observation.
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let first = true;
    const poll = async () => {
      try {
        const next = await controlPort.getStatus();
        if (!cancelled) setStatus(observation(controlPort, next, first));
        first = false;
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
