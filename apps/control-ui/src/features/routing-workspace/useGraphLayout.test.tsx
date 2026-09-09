import { act, renderHook, waitFor } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AudioGraphModel } from '../../core/audio-graph/model';
import type { ControlPort, GraphLayoutResponse } from '../../core/control-port/types';
import { useGraphLayout } from './useGraphLayout';

function model(): AudioGraphModel {
  return {
    revision: 1,
    edges: [],
    nodes: [
      {
        id: 'virtual:input',
        appearance: { title: 'Input', category: 'Source', icon: 'IN' },
        position: { x: 0, y: 0 },
        inputPorts: [],
        outputPorts: [],
      },
      {
        id: 'virtual:output',
        appearance: { title: 'Output', category: 'Output', icon: 'OUT' },
        position: { x: 0, y: 0 },
        inputPorts: [],
        outputPorts: [],
      },
    ],
  };
}
const response: GraphLayoutResponse = {
  graphId: 'a',
  layoutRevision: 4,
  layout: {
    positions: { 'virtual:input': { x: 100, y: 120 }, 'virtual:output': { x: 900, y: -50 } },
    viewport: { x: -200, y: 75, zoom: 0.6 },
  },
};
afterEach(() => vi.useRealTimers());

describe('workspace layout restoration', () => {
  it('loads after the audio model arrives without rewriting saved positions or audio revision', async () => {
    const getGraphLayout = vi.fn().mockResolvedValue(response);
    const setGraphLayout = vi.fn();
    const port = { getGraphLayout, setGraphLayout } as unknown as ControlPort;
    const hook = renderHook(() => {
      const [graph, setGraph] = useState<AudioGraphModel | null>(null);
      return { graph, setGraph, layout: useGraphLayout(port, 'a', graph, setGraph) };
    });
    await waitFor(() => expect(hook.result.current.layout.loaded).toBe(true));
    act(() => hook.result.current.setGraph(model()));
    await waitFor(() => expect(hook.result.current.graph?.nodes[1]?.position.x).toBe(900));
    expect(hook.result.current.graph?.nodes[0]?.position).toEqual({ x: 100, y: 120 });
    expect(hook.result.current.graph?.revision).toBe(1);
    expect(hook.result.current.layout.layout.viewport).toEqual(response.layout.viewport);
    expect(setGraphLayout).not.toHaveBeenCalled();
    hook.unmount();
  });

  it('checkpoints explicit position/Undo and viewport changes, preserving split endpoint IDs', async () => {
    vi.useFakeTimers();
    const getGraphLayout = vi.fn().mockResolvedValue(response);
    const setGraphLayout = vi.fn().mockResolvedValue({ ...response, layoutRevision: 5 });
    const port = { getGraphLayout, setGraphLayout } as unknown as ControlPort;
    const hook = renderHook(() => {
      const [graph, setGraph] = useState<AudioGraphModel | null>(model());
      return useGraphLayout(port, 'a', graph, setGraph);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    act(() => {
      hook.result.current.updatePositions(model());
      hook.result.current.updateViewport({ x: 30, y: 40, zoom: 1.2 });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(setGraphLayout).toHaveBeenCalledExactlyOnceWith('a', {
      expectedLayoutRevision: 4,
      layout: {
        positions: { 'virtual:input': { x: 0, y: 0 }, 'virtual:output': { x: 0, y: 0 } },
        viewport: { x: 30, y: 40, zoom: 1.2 },
      },
    });
    hook.unmount();
  });
});
