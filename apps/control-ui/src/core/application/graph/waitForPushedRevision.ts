import type { GraphRuntimeStatus } from '../../domain/control/graph';
import type { GraphPort } from '../../ports/control/GraphPort';
import type { TelemetryPort } from '../../ports/control/TelemetryPort';

type Port = Pick<TelemetryPort, 'subscribeState'> & Pick<GraphPort, 'getGraphStatus'>;

/** Pending mutations wait for fresh state from the same engine. Reads run before the
 * first push; a stream interruption waits for reconnection within the original deadline. */
export function waitForPushedRevision(
  port: Port,
  revision: number,
  signal: AbortSignal,
  timeoutMs: number,
  failure: (code: string) => Error = (code) => new Error(code),
): Promise<void> {
  return new Promise((resolve, reject) => {
    let done = false;
    let observedPush = false;
    let incarnation: string | undefined;
    let generation = 0;
    let pending = false;
    let unsubscribe: (() => void) | undefined;
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    const timeout = setTimeout(() => finish(failure('graph_apply_timeout')), timeoutMs);
    const finish = (error?: unknown) => {
      done = true;
      unsubscribe?.();
      clearTimeout(timeout);
      clearTimeout(pollTimer);
      signal.removeEventListener('abort', aborted);
      if (error) reject(error);
      else resolve();
    };
    const aborted = () => finish(signal.reason);
    const receive = (status: GraphRuntimeStatus) => {
      if (status.activeRevision > revision || status.submittedRevision > revision)
        finish(failure('graph_revision_stale'));
      else if (status.activeRevision === revision) finish();
    };
    const schedulePoll = (delay = 200) => {
      clearTimeout(pollTimer);
      if (!done && !observedPush && !pending) pollTimer = setTimeout(() => void poll(), delay);
    };
    const poll = async () => {
      pending = true;
      const attempt = generation;
      const current = () => !done && attempt === generation;
      try {
        const status = await port.getGraphStatus();
        if (current()) receive(status);
      } catch (error) {
        if (current()) finish(error);
      }
      pending = false;
      schedulePoll();
    };
    signal.addEventListener('abort', aborted, { once: true });
    if (signal.aborted) {
      aborted();
      return;
    }
    unsubscribe = port.subscribeState?.((state, engineIncarnation) => {
      if (done) return;
      generation += 1;
      if (incarnation && engineIncarnation && incarnation !== engineIncarnation) {
        finish(failure('engine_replaced'));
        return;
      }
      incarnation = engineIncarnation ?? incarnation;
      if (state) {
        observedPush = true;
        receive(state.graph);
      }
      schedulePoll(0);
    });
    if (done) unsubscribe?.();
    else schedulePoll(0);
  });
}
