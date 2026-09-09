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
    this.patch({ busy: false });
  };
  clearError = () => this.patch({ error: null });

  private patch(update: Partial<GraphCatalogState>) {
    this.state = { ...this.state, ...update };
    for (const listener of this.listeners) listener();
  }

  private async run(work: (generation: number) => Promise<void>) {
    if (this.state.busy) return;
    const generation = ++this.generation;
    this.mutationSent = false;
    this.patch({ busy: true, error: null });
    try {
      await work(generation);
    } catch (error) {
      if (generation !== this.generation) return;
      this.patch({
        error: errorCode(error),
        uncertain:
          this.state.uncertain || (this.mutationSent && !(error instanceof ControlPortError)),
      });
    } finally {
      if (generation === this.generation) this.patch({ busy: false });
    }
  }

  private accept(catalog: GraphCatalog, generation: number, replaceDocument: boolean) {
    if (generation !== this.generation) return;
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
    });

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
