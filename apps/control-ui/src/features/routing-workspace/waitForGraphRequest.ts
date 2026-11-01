import type { ControlPort } from '../../core/ports/control/ControlPort';

// First-use consent has a two-minute engine bound; allow ten seconds for its terminal reply.
const PERMISSION_LIMIT_MS = 130_000;
const OBSERVE_MS = 200;
type Port = Pick<ControlPort, 'getGraphStatus' | 'isOperationPending'>;

/** Extends an accepted operation only on fresh permission observations. */
export function waitForGraphRequest<T>(
  request: Promise<T>,
  context: { port: Port; operationId: string; signal: AbortSignal; timeoutMs: number },
  permissionWaiting: ((waiting: boolean) => void) | undefined,
  failure: (code: string) => Error,
): Promise<T> {
  const { port, operationId, signal, timeoutMs } = context;
  return new Promise((resolve, reject) => {
    let done = false;
    let waiting = false;
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    const limit = Date.now() + PERMISSION_LIMIT_MS;
    let timer = setTimeout(() => finish(() => reject(failure('graph_apply_timeout'))), timeoutMs);
    const finish = (settle: () => void) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      clearTimeout(pollTimer);
      signal.removeEventListener('abort', abort);
      if (waiting) permissionWaiting?.(false);
      settle();
    };
    const abort = () => finish(() => reject(signal.reason));
    const updateWaiting = (next: boolean) => {
      if (done) return;
      if (next !== waiting) permissionWaiting?.(next);
      if (next || waiting) {
        clearTimeout(timer);
        timer = setTimeout(
          () => finish(() => reject(failure('graph_apply_timeout'))),
          Math.max(0, Math.min(timeoutMs, limit - Date.now())),
        );
      }
      waiting = next;
    };
    const observe = async () => {
      try {
        const status = await port.getGraphStatus();
        const reason = status.pendingReason === 'awaiting_microphone_permission';
        const accepted = reason && Boolean(await port.isOperationPending?.(operationId));
        updateWaiting(accepted);
      } catch {
        // Failed observations cannot renew the short deadline.
      }
      if (!done) pollTimer = setTimeout(() => void observe(), OBSERVE_MS);
    };
    signal.addEventListener('abort', abort, { once: true });
    if (signal.aborted) abort();
    request.then(
      (value) => finish(() => resolve(value)),
      (error) => finish(() => reject(error)),
    );
    if (!done && port.isOperationPending) pollTimer = setTimeout(() => void observe(), OBSERVE_MS);
  });
}
