import {
  type ControlPort,
  ControlPortError,
  type GraphCatalog,
  type GraphCatalogAction,
  type RuntimeGraphDocument,
} from '../../core/control-port/types';

export interface GraphCatalogState {
  catalog: GraphCatalog | null;
  busy: boolean;
  error: string | null;
  uncertain: boolean;
  /** Only document exchanges change this; renaming must not reset graph undo history. */
  documentGeneration: number;
  document?: RuntimeGraphDocument;
}

function bounded<T>(request: Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('catalog request timed out')), 10_000);
    request.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}

function errorCode(error: unknown): string {
  if (!(error instanceof ControlPortError)) return 'control_request_failed';
  const parameters = error.body.parameters;
  if (
    parameters &&
    typeof parameters === 'object' &&
    'resource' in parameters &&
    parameters.resource === 'graph_catalog'
  ) {
    return 'graph_catalog_unavailable';
  }
  return error.body.code;
}

function workspaceRejected(error: ControlPortError): boolean {
  const parameters = error.body.parameters;
  return Boolean(
    parameters &&
      typeof parameters === 'object' &&
      'workspaceOutcome' in parameters &&
      parameters.workspaceOutcome === 'not_applied',
  );
}

function workspaceUncertain(error: unknown) {
  return !(error instanceof ControlPortError) || (error.status >= 500 && !workspaceRejected(error));
}

function retryableRead(error: unknown) {
  if (!(error instanceof ControlPortError)) return true;
  if (errorCode(error) === 'graph_catalog_unavailable') return false;
  return [408, 429, 502, 503, 504].includes(error.status);
}

function mutationUncertain(error: unknown) {
  if (!(error instanceof ControlPortError)) return true;
  if (error.status < 500 || workspaceRejected(error)) return false;
  const parameters = error.body.parameters;
  return !(
    parameters &&
    typeof parameters === 'object' &&
    'catalogOutcome' in parameters &&
    parameters.catalogOutcome === 'not_applied'
  );
}

/** One writer, confirmed audio activation, and generation fencing for late IPC replies. */
export class GraphCatalogController {
  private state: GraphCatalogState = {
    catalog: null,
    busy: false,
    error: null,
    uncertain: false,
    documentGeneration: 0,
  };
  private generation = 0;
  private mutationSent = false;
  private retryTimer: ReturnType<typeof setTimeout> | undefined;
  private retryDelay = 500;
  private readonly listeners = new Set<() => void>();

  constructor(private readonly port: ControlPort) {}

  snapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  dispose = () => {
    this.generation += 1;
    clearTimeout(this.retryTimer);
    this.patch({ busy: false });
  };
  clearError = () => this.patch({ error: null });

  private patch(update: Partial<GraphCatalogState>) {
    this.state = { ...this.state, ...update };
    for (const listener of this.listeners) listener();
  }

  private async run(work: (generation: number) => Promise<void>, retryRead = false) {
    if (this.state.busy) return;
    clearTimeout(this.retryTimer);
    const generation = ++this.generation;
    this.mutationSent = false;
    this.patch({ busy: true, error: null });
    try {
      await work(generation);
    } catch (error) {
      if (generation !== this.generation) return;
      this.patch({
        error: errorCode(error),
        uncertain: this.state.uncertain || (this.mutationSent && mutationUncertain(error)),
      });
      if (retryRead && !this.state.catalog && retryableRead(error)) {
        this.retryTimer = setTimeout(() => {
          // Every operation and disposal clears this timer before changing its generation.
          void this.reload();
        }, this.retryDelay);
        this.retryDelay = Math.min(5000, this.retryDelay * 2);
      }
    } finally {
      if (generation === this.generation) this.patch({ busy: false });
    }
  }

  private accept(catalog: GraphCatalog, generation: number, replaceDocument: boolean) {
    if (generation !== this.generation) return;
    this.retryDelay = 500;
    this.patch({
      catalog,
      error: null,
      uncertain: false,
      document: replaceDocument ? catalog.document : this.state.document,
      documentGeneration: this.state.documentGeneration + (replaceDocument ? 1 : 0),
    });
  }

  reload = () =>
    this.run(async (generation) => {
      if (!this.port.getGraphCatalog) throw new Error('catalog unavailable');
      this.accept(await bounded(this.port.getGraphCatalog()), generation, true);
    }, Boolean(this.port.getGraphCatalog));

  workspaceFile = (action: 'new' | 'open' | 'save' | 'save_as', locale: string) =>
    this.run(async (generation) => {
      const catalog = this.state.catalog;
      if (!catalog || this.state.uncertain || !this.port.workspaceFile) return;
      const status = await bounded(this.port.getGraphStatus());
      if (generation !== this.generation) return;
      this.mutationSent = true;
      const next = await this.port
        .workspaceFile({
          action,
          locale,
          expectedRevision: status.submittedRevision,
          expectedCatalogRevision: catalog.catalogRevision,
        })
        .catch((error: unknown) => {
          if (generation === this.generation && workspaceUncertain(error))
            this.patch({ uncertain: true });
          throw error;
        });
      // A native file dialog can outlive this editor owner. Its reply must not
      // mutate the replacement view or begin another catalog synchronization.
      if (generation !== this.generation) return;
      if (!next) {
        this.mutationSent = false;
        return;
      }
      this.accept(next, generation, action === 'new' || action === 'open');
      await this.confirmWorkspaceFile(next, generation);
    });

  private async confirmWorkspaceFile(next: GraphCatalog, generation: number) {
    if (generation !== this.generation) return;
    this.patch({ uncertain: true });
    await this.waitActive(next.document.revision, generation);
    if (generation !== this.generation) return;
    // The accepted document remains visible even if capture synchronization fails.
    const confirmed = this.port.getGraphCatalog ? await this.port.getGraphCatalog() : next;
    this.accept(confirmed, generation, false);
  }

  private async waitActive(revision: number, generation: number) {
    const deadline = Date.now() + 10_000;
    while (generation === this.generation) {
      const status = await bounded(this.port.getGraphStatus());
      if (status.submittedRevision !== revision) throw new Error('graph changed during activation');
      if (status.activeRevision === revision) return;
      if (Date.now() >= deadline) throw new Error('graph activation timed out');
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }

  mutate = async (action: GraphCatalogAction) => {
    const catalog = this.state.catalog;
    if (!catalog || this.state.uncertain) return;
    await this.run(async (generation) => {
      if (!this.port.mutateGraphCatalog) throw new Error('catalog mutations unavailable');
      const document = await bounded(this.port.getGraph());
      if (generation !== this.generation) return;
      this.mutationSent = true;
      const next = await bounded(
        this.port.mutateGraphCatalog({
          ...action,
          expectedRevision: document.revision,
          expectedCatalogRevision: catalog.catalogRevision,
        }),
      );
      await this.waitActive(next.document.revision, generation);
      this.accept(next, generation, action.action !== 'rename');
    });
  };
}
