import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  ExternalProcessorCatalog,
  ExternalProcessorDescriptor,
  ExternalProcessorDiagnostic,
} from '../../core/domain/control/processors';
import type { InventoryPort } from '../../core/ports/control/InventoryPort';

const PATHS_KEY = 'mixrelay.processorPaths.v1';
type CatalogAction = 'initial' | 'scan' | 'directory';
type CatalogPort = Pick<
  InventoryPort,
  'getProcessors' | 'rescanProcessors' | 'selectProcessorDirectory'
>;
export type ProcessorCatalogFailure = 'catalog' | 'directory' | 'paths';
interface CatalogState {
  processors: ExternalProcessorDescriptor[];
  diagnostics: ExternalProcessorDiagnostic[];
  failure: ProcessorCatalogFailure | null;
  busy: boolean;
}
export interface ProcessorCatalog extends CatalogState {
  refresh: (directory: boolean) => void;
  retry: () => void;
}
const initialState: CatalogState = {
  processors: [],
  diagnostics: [],
  failure: null,
  busy: true,
};

/** One port owns one serialized discovery session. Disposal invalidates every native promise. */
class CatalogSession {
  private cancelled = false;
  private running = false;
  private failedAction: CatalogAction = 'initial';
  private phase: ProcessorCatalogFailure = 'catalog';
  private paths: string[] = [];

  constructor(
    private port: CatalogPort,
    private update: (patch: Partial<CatalogState>) => void,
  ) {}

  private async preparePaths(action: CatalogAction): Promise<boolean> {
    if (action === 'initial') {
      this.phase = 'paths';
      const saved: unknown = JSON.parse(window.localStorage.getItem(PATHS_KEY) ?? '[]');
      if (!Array.isArray(saved) || !saved.every((path) => typeof path === 'string')) {
        throw new Error('Invalid processor search paths');
      }
      this.paths = saved;
    }
    if (action !== 'directory') return true;
    this.phase = 'directory';
    if (!this.port.selectProcessorDirectory) throw new Error('Directory picker unavailable');
    const selected = await this.port.selectProcessorDirectory();
    if (this.cancelled || !selected) return false;
    this.phase = 'paths';
    const next = [...new Set([...this.paths, selected])];
    window.localStorage.setItem(PATHS_KEY, JSON.stringify(next));
    this.paths = next;
    return true;
  }

  private accept(catalog: ExternalProcessorCatalog) {
    if (this.cancelled) return;
    const diagnostics = catalog.diagnostics ?? [];
    // A GET can return the last failed discovery. Retry must execute a new scan.
    if (diagnostics.length) this.failedAction = 'scan';
    this.update({ processors: catalog.processors, diagnostics, failure: null });
  }

  async run(action = this.failedAction) {
    if (this.cancelled || this.running) return;
    this.running = true;
    this.failedAction = action;
    this.update({ busy: true });
    try {
      if (!(await this.preparePaths(action)) || this.cancelled) return;
      this.phase = 'catalog';
      if (action === 'directory') this.failedAction = 'scan';
      const catalog = await (action === 'initial' && !this.paths.length
        ? this.port.getProcessors()
        : this.port.rescanProcessors(this.paths));
      this.accept(catalog);
    } catch {
      if (!this.cancelled) this.update({ failure: this.phase });
    } finally {
      this.running = false;
      if (!this.cancelled) this.update({ busy: false });
    }
  }

  dispose() {
    this.cancelled = true;
  }
}

/** Preserve the last catalog on failure; cancellation never pretends a scan succeeded. */
export function useProcessorCatalog(
  controlPort: CatalogPort,
  suppliedCatalog?: ProcessorCatalog,
): ProcessorCatalog {
  const owned = suppliedCatalog === undefined;
  const [state, setState] = useState(initialState);
  const request = useRef<((action?: CatalogAction) => void) | null>(null);
  const refresh = useCallback(
    (directory: boolean) => request.current?.(directory ? 'directory' : 'scan'),
    [],
  );
  const retry = useCallback(() => request.current?.(), []);
  useEffect(() => {
    if (!owned) return;
    setState(initialState);
    const session = new CatalogSession(controlPort, (patch) =>
      setState((current) => ({ ...current, ...patch })),
    );
    request.current = (action) => {
      void session.run(action);
    };
    void session.run('initial');
    return () => {
      session.dispose();
      request.current = null;
    };
  }, [controlPort, owned]);
  return suppliedCatalog ?? { ...state, refresh, retry };
}
