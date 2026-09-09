import {
  type Dispatch,
  type SetStateAction,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
} from 'react';
import type { AudioGraphModel } from '../../core/audio-graph/model';
import type { ControlPort, GraphLayout } from '../../core/control-port/types';
import { GraphLayoutController } from './GraphLayoutController';

export function useGraphLayout(
  port: ControlPort,
  graphId: string | undefined,
  model: AudioGraphModel | null,
  setModel: Dispatch<SetStateAction<AudioGraphModel | null>>,
) {
  const controller = useMemo(() => new GraphLayoutController(port, graphId), [port, graphId]);
  const state = useSyncExternalStore(controller.subscribe, controller.snapshot);
  const restored = useRef({ controller, version: 0 });
  useEffect(() => {
    void controller.reload();
    return controller.dispose;
  }, [controller]);
  useEffect(() => {
    if (
      !model ||
      !state.restoreVersion ||
      (restored.current.controller === controller &&
        restored.current.version === state.restoreVersion)
    )
      return;
    restored.current = { controller, version: state.restoreVersion };
    // Layout owns positions only; a concurrent catalog refresh owns current node metadata.
    setModel(
      (current) =>
        current && {
          ...current,
          nodes: current.nodes.map((node) => ({
            ...node,
            position: state.layout.positions[node.id] ?? node.position,
          })),
        },
    );
  }, [controller, model, setModel, state.layout.positions, state.restoreVersion]);
  const updatePositions = useCallback(
    (next: AudioGraphModel) => {
      controller.update({
        ...controller.snapshot().layout,
        positions: Object.fromEntries(next.nodes.map((node) => [node.id, { ...node.position }])),
      });
    },
    [controller],
  );
  const updateViewport = useCallback(
    (viewport: NonNullable<GraphLayout['viewport']>) => {
      controller.update({ ...controller.snapshot().layout, viewport });
    },
    [controller],
  );
  return { ...state, controller, updatePositions, updateViewport };
}
