import type { AudioGraphModel } from '../../core/audio-graph/model';
import {
  type ControlPort,
  ControlPortError,
  type NodeControlRequest,
  type ProcessorParameterChange,
  type RuntimeGraphDocument,
} from '../../core/control-port/types';
import { audioGraphToRuntimeGraph } from './model';

export const GRAPH_APPLICATION_TIMEOUT_MS = 10_000;
const POLL_MS = 200;

interface QueueCallbacks {
  getGraphId?: () => string | undefined;
  beforeApply?: () => Promise<void>;
  beforeReload?: () => Promise<void>;
  getDocument: () => RuntimeGraphDocument | null;
  getModel: () => AudioGraphModel | null;
  document: (document: RuntimeGraphDocument) => void;
  restore: (document: RuntimeGraphDocument) => void;
  reconcile?: (before: RuntimeGraphDocument, after: RuntimeGraphDocument) => boolean;
  state: (state: 'active' | 'applying' | 'blocked') => void;
  error: (code: string | null) => void;
  persistenceWarning?: (code: string | null) => void;
}

/** A resolved request is not necessarily an applied edit. Uncertain commands must not be replayed. */
export type NodeControlResult =
  | { state: 'applied'; revision: number }
  | { state: 'rejected'; code: string }
  | { state: 'uncertain'; code: string };

function failedNodeControl(error: unknown, sent: boolean, accepted: boolean): NodeControlResult {
  return {
    state: !sent || (!accepted && definiteNodeRejection(error)) ? 'rejected' : 'uncertain',
    code: errorCode(error),
  };
}

class GraphApplicationError extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}

function failure(code: string) {
  return new GraphApplicationError(code);
}

function errorCode(error: unknown): string {
  if (error instanceof GraphApplicationError) return error.code;
  return error instanceof ControlPortError ? error.body.code : 'control_request_failed';
}

function definiteNodeRejection(error: unknown) {
  if (error instanceof ControlPortError) {
    const parameters = error.body.parameters;
    if (
      typeof parameters === 'object' &&
      parameters !== null &&
      'nodeControlOutcome' in parameters &&
      parameters.nodeControlOutcome === 'not_applied'
    )
      return true;
  }
  return (
    (error instanceof ControlPortError || error instanceof GraphApplicationError) &&
    ['node_not_found', 'node_disabled', 'graph_revision_stale'].includes(errorCode(error))
  );
}

function hasSameProcessor(graph: RuntimeGraphDocument, change: ProcessorParameterChange) {
  return graph.nodes.some((node) => {
    const config = node.config;
    return (
      node.id === change.nodeId &&
      node.type === 'provider' &&
      typeof config === 'object' &&
      config !== null &&
      'typeId' in config &&
      config.typeId === change.typeId
    );
  });
}

/** Bounds even a stalled IPC promise. Late replies cannot mutate the queue after cancellation. */
function bounded<T>(request: Promise<T>, signal: AbortSignal, timeoutMs: number): Promise<T> {
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

function pause(signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const cancel = () => {
      clearTimeout(timer);
      reject(signal.reason);
    };
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', cancel);
      resolve();
    }, POLL_MS);
    signal.addEventListener('abort', cancel, { once: true });
    if (signal.aborted) cancel();
  });
}

/** One writer per workspace. Failures retain the editable draft and require an explicit decision. */
export class GraphApplicationQueue {
  private operation: AbortController | null = null;
  private pending: AudioGraphModel | null = null;
  private pendingParameters = new Map<string, ProcessorParameterChange>();
  private blocked = false;
  private document: RuntimeGraphDocument | null = null;

  constructor(
    private readonly port: Pick<
      ControlPort,
      'applyGraph' | 'getGraph' | 'getGraphStatus' | 'updateNodeControl'
    >,
    private readonly callbacks: QueueCallbacks,
  ) {}

  get canPublishParameters() {
    return !this.blocked && !this.operation;
  }

  reset(document: RuntimeGraphDocument | null = null) {
    this.operation?.abort();
    this.operation = null;
    this.pending = null;
    this.pendingParameters.clear();
    this.blocked = false;
    this.document = document;
  }

  submit(model: AudioGraphModel, changes: ProcessorParameterChange[] = []) {
    this.pending = model;
    for (const change of changes)
      this.pendingParameters.set(
        JSON.stringify([change.nodeId, change.typeId, change.parameterId]),
        change,
      );
    if (!this.blocked && !this.operation) void this.run('submit');
  }

  retry() {
    if (!this.operation) void this.run('retry');
  }

  reload() {
    if (!this.operation) void this.run('reload');
  }

  refresh() {
    if (this.canPublishParameters) void this.run('refresh');
  }

  /** Shares the graph writer and CAS baseline; never displays an unacknowledged power state. */
  async setNodeControl(
    graphId: string,
    nodeId: string,
    change: Omit<NodeControlRequest, 'expectedRevision'>,
    expectedRevision?: number,
  ): Promise<NodeControlResult> {
    if (!this.port.updateNodeControl) return { state: 'rejected', code: 'control_unavailable' };
    const source = this.document ?? this.callbacks.getDocument();
    const rejection = this.nodeControlRejection(graphId, expectedRevision);
    if (rejection) return { state: 'rejected', code: rejection };
    let sent = false;
    let accepted = false;
    const operation = new AbortController();
    const { signal } = operation;
    this.operation = operation;
    this.callbacks.state('applying');
    this.callbacks.error(null);
    try {
      if (this.callbacks.beforeApply)
        await bounded(this.callbacks.beforeApply(), signal, GRAPH_APPLICATION_TIMEOUT_MS);
      signal.throwIfAborted();
      this.validateNodeControlSource(source, graphId, nodeId);
      sent = true;
      const response = await bounded(
        this.port.updateNodeControl(graphId, nodeId, {
          ...change,
          expectedRevision: source.revision,
        }),
        signal,
        GRAPH_APPLICATION_TIMEOUT_MS,
      );
      accepted = true;
      if (response.revision !== source.revision + 1) throw failure('graph_revision_stale');
      if (response.state !== 'active') await this.waitForRevision(response.revision, signal);
      const document = await this.readEngine(signal, true, {
        graphId,
        revision: response.revision,
      });
      if (this.pending) throw failure('graph_draft_changed');
      this.acceptControls(source, document);
      this.callbacks.persistenceWarning?.(response.persistenceError ?? null);
      this.callbacks.state('active');
      return { state: 'applied', revision: response.revision };
    } catch (error) {
      if (!signal.aborted) await this.recoverNodeControl(error, signal, graphId, accepted);
      return failedNodeControl(error, sent, accepted);
    } finally {
      if (this.operation === operation) this.operation = null;
    }
  }

  private nodeControlRejection(graphId: string, expectedRevision?: number) {
    if (!this.canPublishParameters) return 'graph_exchange_busy';
    if (this.callbacks.getGraphId?.() !== graphId) return 'graph_revision_stale';
    const source = this.document ?? this.callbacks.getDocument();
    if (expectedRevision !== undefined && source?.revision !== expectedRevision)
      return 'graph_revision_stale';
    return null;
  }

  private acceptControls(before: RuntimeGraphDocument, after: RuntimeGraphDocument) {
    if (this.callbacks.reconcile) {
      if (!this.callbacks.reconcile(before, after)) throw failure('graph_revision_stale');
    } else this.callbacks.restore(after);
  }

  private validateNodeControlSource(
    source: RuntimeGraphDocument | null,
    graphId: string,
    nodeId: string,
  ): asserts source is RuntimeGraphDocument {
    if (!source?.nodes.some((node) => node.id === nodeId)) throw failure('node_not_found');
    const current = this.document ?? this.callbacks.getDocument();
    if (current?.revision !== source.revision || this.callbacks.getGraphId?.() !== graphId)
      throw failure('graph_revision_stale');
  }

  private async recoverNodeControl(
    error: unknown,
    signal: AbortSignal,
    graphId: string,
    accepted: boolean,
  ) {
    if (this.callbacks.getGraphId?.() !== graphId) return;
    if (!accepted && !this.pending && definiteNodeRejection(error)) {
      try {
        const before = this.document ?? this.callbacks.getDocument();
        const document = await this.readEngine(signal, true, { graphId });
        if (!this.pending && before) {
          this.acceptControls(before, document);
          this.callbacks.error(errorCode(error));
          this.callbacks.state('active');
          return;
        }
      } catch {
        if (signal.aborted) return;
      }
    }
    await this.recover(error, signal, graphId);
  }

  private async waitForRevision(revision: number, signal: AbortSignal) {
    const deadline = Date.now() + GRAPH_APPLICATION_TIMEOUT_MS;
    while (!signal.aborted) {
      const remaining = deadline - Date.now();
      if (remaining <= 0) throw failure('graph_apply_timeout');
      const status = await bounded(this.port.getGraphStatus(), signal, remaining);
      signal.throwIfAborted();
      // A newer document is not confirmation of our edit: do not overwrite another writer.
      if (status.activeRevision > revision || status.submittedRevision > revision) {
        throw failure('graph_revision_stale');
      }
      if (status.activeRevision === revision) return;
      await pause(signal);
    }
    throw signal.reason;
  }

  private async readEngine(
    signal: AbortSignal,
    wait: boolean,
    expected?: { graphId: string; revision?: number },
  ) {
    const document = await bounded(this.port.getGraph(), signal, GRAPH_APPLICATION_TIMEOUT_MS);
    if (wait) await this.waitForRevision(document.revision, signal);
    signal.throwIfAborted();
    if (
      expected &&
      (this.callbacks.getGraphId?.() !== expected.graphId ||
        (expected.revision !== undefined && document.revision !== expected.revision))
    )
      throw failure('graph_revision_stale');
    this.document = document;
    this.callbacks.document(document);
    return document;
  }

  private async apply(
    model: AudioGraphModel,
    changes: ProcessorParameterChange[],
    signal: AbortSignal,
  ) {
    // A previously accepted UI parameter edit must settle before native state is captured.
    if (this.callbacks.beforeApply)
      await bounded(this.callbacks.beforeApply(), signal, GRAPH_APPLICATION_TIMEOUT_MS);
    signal.throwIfAborted();
    const source = this.document ?? this.callbacks.getDocument();
    if (!source) throw failure('control_request_failed');
    const converted = audioGraphToRuntimeGraph(model, source);
    // Only advance the acknowledged engine baseline. Local undo/edit revisions must
    // not jump over a concurrent writer and accidentally bypass stale-revision checks.
    const document = { ...converted, revision: source.revision + 1 };
    const surviving = changes.filter(
      (change) => hasSameProcessor(source, change) && hasSameProcessor(document, change),
    );
    const response = await bounded(
      this.port.applyGraph(
        surviving.length ? { ...document, processorParameterChanges: surviving } : document,
      ),
      signal,
      GRAPH_APPLICATION_TIMEOUT_MS,
    );
    if (response.revision !== document.revision) throw failure('graph_revision_stale');
    if (response.state !== 'active') await this.waitForRevision(response.revision, signal);
    signal.throwIfAborted();
    // Persistence is a separate acknowledgement: audio has already applied. Do not
    // retry/rollback the graph or block subsequent edits because disk storage failed.
    this.callbacks.persistenceWarning?.(response.persistenceError ?? null);
    this.document = document;
    this.callbacks.document(document);
  }

  private async recover(error: unknown, signal: AbortSignal, graphId?: string) {
    this.blocked = true;
    // Refresh the engine baseline, never overwrite the user's draft/history on a failed edit.
    // A failed read leaves the queue blocked; retry/reload must read it again before writing.
    try {
      await this.readEngine(signal, false, graphId ? { graphId } : undefined);
    } catch {
      if (signal.aborted) return;
    }
    if (signal.aborted) return;
    if (graphId && this.callbacks.getGraphId?.() !== graphId) return;
    this.callbacks.error(errorCode(error));
    this.callbacks.state('blocked');
  }

  private async applyPending(model: AudioGraphModel, signal: AbortSignal) {
    const parameters = this.pendingParameters;
    this.pendingParameters = new Map();
    this.pending = null;
    try {
      await this.apply(model, [...parameters.values()], signal);
    } catch (error) {
      // Keep unacknowledged intent for explicit Retry. A newer queued edit wins per ID.
      if (!signal.aborted)
        this.pendingParameters = new Map([...parameters, ...this.pendingParameters]);
      throw error;
    }
  }

  private async prepareRecovery(
    mode: 'retry' | 'reload' | 'refresh',
    signal: AbortSignal,
    initialModel: AudioGraphModel | null,
  ) {
    if (mode === 'reload' && this.callbacks.beforeReload)
      await bounded(this.callbacks.beforeReload(), signal, GRAPH_APPLICATION_TIMEOUT_MS);
    const before = this.document ?? this.callbacks.getDocument();
    const graphId = this.callbacks.getGraphId?.();
    const document = await this.readEngine(signal, true, graphId ? { graphId } : undefined);
    if (mode === 'refresh') {
      // Layout edits may continue while reading; reconciliation uses the latest model.
      // An audio edit queued meanwhile requires explicit recovery, never a silent overwrite.
      if (!before || this.pending) throw failure('graph_draft_changed');
      this.acceptControls(before, document);
      return;
    }
    if (mode === 'reload') {
      if (this.callbacks.getModel() !== initialModel) throw failure('graph_draft_changed');
      this.pending = null;
      this.pendingParameters.clear();
      this.callbacks.restore(document);
    } else {
      this.pending = this.callbacks.getModel();
    }
    this.blocked = false;
  }

  private async run(mode: 'submit' | 'retry' | 'reload' | 'refresh') {
    const operation = new AbortController();
    const { signal } = operation;
    this.operation = operation;
    this.callbacks.state('applying');
    this.callbacks.error(null);
    const initialModel = this.callbacks.getModel();
    try {
      if (mode !== 'submit') await this.prepareRecovery(mode, signal, initialModel);
      while (this.pending && !signal.aborted) {
        await this.applyPending(this.pending, signal);
      }
      if (!signal.aborted) this.callbacks.state('active');
    } catch (error) {
      if (!signal.aborted) await this.recover(error, signal);
    } finally {
      if (this.operation === operation) this.operation = null;
    }
  }
}
