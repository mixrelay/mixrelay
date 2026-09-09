import type { ControlPort, GraphLayout, GraphLayoutResponse } from '../../core/control-port/types';

export interface LayoutState {
  layout: GraphLayout;
  loaded: boolean;
  loading: boolean;
  saving: boolean;
  dirty: boolean;
  error: boolean;
  restoreVersion: number;
}

function bounded<T>(request: Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('layout request timed out')), 10_000);
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

/** Graph-scoped single writer. Conflicts never silently replace another window's layout. */
export class GraphLayoutController {
  private state: LayoutState;
  private revision = 0;
  private generation = 0;
  private disposed = false;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private readonly listeners = new Set<() => void>();
  readonly enabled: boolean;

  constructor(
    private readonly port: ControlPort,
    private readonly graphId?: string,
  ) {
    this.enabled = !!(graphId && port.getGraphLayout && port.setGraphLayout);
    this.state = {
      layout: { positions: {} },
      loaded: !this.enabled,
      loading: false,
      saving: false,
      dirty: false,
      error: false,
      restoreVersion: 0,
    };
  }

  snapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private patch(update: Partial<LayoutState>) {
    this.state = { ...this.state, ...update };
    for (const listener of this.listeners) listener();
  }
  private cancelTimer() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
  }
  dispose = () => {
    this.disposed = true;
    this.generation++;
    this.cancelTimer();
  };

  private validate(response: GraphLayoutResponse) {
    if (response.graphId !== this.graphId) throw new Error('layout graph identity mismatch');
    return response;
  }

  /** Explicit reload discards only presentation edits, never audio graph/Undo state. */
  reload = async () => {
    if (!this.enabled || !this.graphId || !this.port.getGraphLayout) return;
    // React StrictMode may set up this same owner again after its cleanup probe.
    this.disposed = false;
    this.cancelTimer();
    const generation = ++this.generation;
    this.patch({ loading: true, saving: false, error: false });
    try {
      const response = this.validate(await bounded(this.port.getGraphLayout(this.graphId)));
      if (generation !== this.generation) return;
      this.revision = response.layoutRevision;
      this.patch({
        layout: response.layout,
        loaded: true,
        dirty: false,
        restoreVersion: this.state.restoreVersion + 1,
      });
    } catch {
      if (generation === this.generation) this.patch({ error: true });
    } finally {
      if (generation === this.generation) this.patch({ loading: false });
    }
  };

  update = (layout: GraphLayout) => {
    if (this.disposed || !this.enabled || !this.state.loaded || this.state.loading) return;
    if (JSON.stringify(layout) === JSON.stringify(this.state.layout)) return;
    this.patch({ layout, dirty: true });
    this.cancelTimer();
    if (!this.state.error)
      this.timer = setTimeout(() => {
        void this.save();
      }, 400);
  };

  private async save() {
    this.cancelTimer();
    if (!this.graphId || !this.port.setGraphLayout || this.state.saving || !this.state.dirty)
      return;
    const generation = this.generation;
    const layout = this.state.layout;
    this.patch({ saving: true });
    try {
      const response = this.validate(
        await bounded(
          this.port.setGraphLayout(this.graphId, {
            expectedLayoutRevision: this.revision,
            layout,
          }),
        ),
      );
      if (generation !== this.generation) return;
      this.revision = response.layoutRevision;
      this.patch({ dirty: this.state.layout !== layout, error: false });
    } catch {
      if (generation === this.generation) this.patch({ error: true });
    } finally {
      if (generation === this.generation) {
        this.patch({ saving: false });
        if (this.state.dirty && !this.state.error) void this.save();
      }
    }
  }

  /** Only the explicit "Save current layout" action rebases a conflicting local draft. */
  retry = async () => {
    if (this.disposed || this.state.loading || this.state.saving) return;
    if (!this.state.dirty) return this.reload();
    if (!this.graphId || !this.port.getGraphLayout) return;
    const generation = ++this.generation;
    this.patch({ loading: true });
    try {
      const response = this.validate(await bounded(this.port.getGraphLayout(this.graphId)));
      if (generation !== this.generation) return;
      this.revision = response.layoutRevision;
      this.patch({ error: false });
    } catch {
      if (generation === this.generation) this.patch({ error: true });
    } finally {
      if (generation === this.generation) {
        this.patch({ loading: false });
        if (!this.state.error) void this.save();
      }
    }
  };
}
