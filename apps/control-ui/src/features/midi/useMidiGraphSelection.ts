import { useContext, useEffect } from 'react';
import { MidiContext } from '../../core/midi/MidiPort';
import type { useGraphCatalog } from '../graphs/useGraphCatalog';

export function useMidiGraphSelection(
  collection: Pick<
    ReturnType<typeof useGraphCatalog>,
    'catalog' | 'busy' | 'uncertain' | 'error' | 'mutate'
  >,
) {
  const graphId = useContext(MidiContext)?.selected?.target.graphId;
  const { catalog, busy, uncertain, error, mutate } = collection;
  useEffect(() => {
    if (
      graphId &&
      catalog &&
      graphId !== catalog.activeId &&
      catalog.graphs.some((graph) => graph.id === graphId) &&
      !busy &&
      !uncertain &&
      !error
    ) {
      void mutate({ action: 'select', id: graphId });
    }
  }, [graphId, catalog, busy, uncertain, error, mutate]);
}
