import type { ControlPort } from '../../core/control-port/types';
import { waitForPushedRevision } from '../../core/control-port/waitForPushedRevision';

const OBSERVATION_MS = 200;
type ReadPort = Pick<
  ControlPort,
  'getGraph' | 'getGraphCatalog' | 'getGraphStatus' | 'subscribeState'
>;

function boundedGraphRequest<T>(
  request: Promise<T>,
  signal: AbortSignal,
  timeoutMs: number,
  failure: (code: string) => Error,
): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => finish(() => reject(failure('graph_apply_timeout'))), timeoutMs);
    const cancel = () => finish(() => reject(signal.reason));
    const finish = (settle: () => void) => {
      clearTimeout(timer);
      signal.removeEventListener('abort', cancel);
      settle();
    };
    signal.addEventListener('abort', cancel, { once: true });
    if (signal.aborted) cancel();
    request.then(
      (value) => finish(() => resolve(value)),
      (error) => finish(() => reject(error)),
    );
  });
}

export function pauseGraphObservation(signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const cancel = () => {
      clearTimeout(timer);
      reject(signal.reason);
    };
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', cancel);
      resolve();
    }, OBSERVATION_MS);
    signal.addEventListener('abort', cancel, { once: true });
  });
}

export class GraphReadback {
  constructor(
    private readonly port: ReadPort,
    private readonly timeoutMs: number,
    private readonly failure: (code: string) => Error,
  ) {}

  bound<T>(request: Promise<T>, signal: AbortSignal, timeoutMs = this.timeoutMs) {
    return boundedGraphRequest(request, signal, timeoutMs, this.failure);
  }

  async waitForRevision(revision: number, signal: AbortSignal) {
    if (this.port.subscribeState)
      return waitForPushedRevision(this.port, revision, signal, this.timeoutMs, this.failure);
    const deadline = Date.now() + this.timeoutMs;
    while (true) {
      signal.throwIfAborted();
      const remaining = deadline - Date.now();
      if (remaining <= 0) throw this.failure('graph_apply_timeout');
      const status = await this.bound(this.port.getGraphStatus(), signal, remaining);
      signal.throwIfAborted();
      // A newer document is not confirmation of our edit: do not overwrite another writer.
      if (status.activeRevision > revision || status.submittedRevision > revision) {
        throw this.failure('graph_revision_stale');
      }
      if (status.activeRevision === revision) return;
      await pauseGraphObservation(signal);
    }
  }

  /**
   * Reads this graph's document only while it is still selected. Another client can select a
   * different graph without changing the audio revision, and the engine publishes the new
   * document before its catalog, so separate reads cannot attribute `GET /graph` to a graph.
   * The catalog response names the selection and carries its document in one snapshot; a
   * lagging revision there only makes the next write stale, never attributes B's content to A.
   */
  async document(signal: AbortSignal, graphId: string | undefined) {
    if (!this.port.getGraphCatalog || graphId === undefined)
      return this.bound(this.port.getGraph(), signal);
    const catalog = await this.bound(this.port.getGraphCatalog(), signal);
    if (catalog.activeId !== graphId) throw this.failure('graph_catalog_revision_stale');
    return catalog.document;
  }
}
