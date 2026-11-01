import { useEffect, useState } from 'react';
import type { MixRelayStatus } from '../../core/domain/control/status';
import { structuralShare } from '../../core/domain/structuralShare';
import type { ControlPort } from '../../core/ports/control/ControlPort';

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
    let pushed = false;
    let generation = 0;
    const unsubscribe = controlPort.subscribeState?.((state) => {
      generation += 1;
      pushed = Boolean(state);
      if (state) {
        setStatus(observation(controlPort, state.status, first));
        first = false;
      } else setStatus(null);
    });
    const schedule = () => {
      if (!cancelled) timer = setTimeout(poll, 1000);
    };
    const poll = async () => {
      const attempt = generation;
      const current = () => !cancelled && attempt === generation;
      if (pushed) {
        schedule();
        return;
      }
      try {
        const next = await controlPort.getStatus();
        if (current()) setStatus(observation(controlPort, next, first));
        first = false;
      } catch {
        // Do not leave stale green health visible after the control service disappears.
        if (current()) setStatus(null);
      } finally {
        schedule();
      }
    };
    void poll();
    return () => {
      cancelled = true;
      unsubscribe?.();
      clearTimeout(timer);
    };
  }, [controlPort, refreshToken]);
  // Refreshing the same service retains its last observation until the reply.
  // A replacement service or a failed read must still hide the old health.
  return status?.controlPort === controlPort ? status.value : null;
}
