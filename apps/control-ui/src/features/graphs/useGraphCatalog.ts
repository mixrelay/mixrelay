import { useEffect, useMemo, useState, useSyncExternalStore } from 'react';
import { type CatalogPort, GraphCatalogController } from './GraphCatalogController';

export function useGraphCatalog(controlPort: CatalogPort) {
  const controller = useMemo(() => new GraphCatalogController(controlPort), [controlPort]);
  const state = useSyncExternalStore(
    controller.subscribe,
    controller.snapshot,
    controller.snapshot,
  );
  useEffect(() => {
    void controller.reload();
    return controller.dispose;
  }, [controller]);
  return {
    ...state,
    mutate: controller.mutate,
    workspaceFile: controller.workspaceFile,
    reload: controller.reload,
    clearError: controller.clearError,
  };
}

export function useCatalogTitle(
  collection: ReturnType<typeof useGraphCatalog>,
  busy: boolean,
  fallbackName: string,
) {
  const selected = collection.catalog?.graphs.find(
    (graph) => graph.id === collection.catalog?.activeId,
  );
  const name = selected?.name ?? '';
  const [draft, setDraft] = useState(name);
  useEffect(() => setDraft(name), [name]);
  const normalized = draft.trim() || fallbackName;
  const unsaved = !!selected && normalized !== name;
  useEffect(() => {
    if (!selected || !unsaved || busy || collection.error) return;
    const timer = setTimeout(() => {
      void collection.mutate({ action: 'rename', id: selected.id, name: normalized });
    }, 500);
    return () => clearTimeout(timer);
  }, [selected, unsaved, busy, collection.error, collection.mutate, normalized]);
  return {
    draft,
    unsaved,
    setDraft: (value: string) => {
      setDraft(value);
      collection.clearError();
    },
  };
}
