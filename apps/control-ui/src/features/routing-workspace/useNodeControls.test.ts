import { act, renderHook } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { ControlPort, GraphNodeControls } from '../../core/control-port/types';
import type { GraphApplicationQueue } from './GraphApplicationQueue';
import { useNodeControls } from './useNodeControls';

afterEach(() => vi.useRealTimers());
const snapshot: GraphNodeControls = { graphId: 'a', revision: 2, nodes: [] };
function queue() {
  return { canPublishParameters: true, refresh: vi.fn() } as unknown as GraphApplicationQueue;
}
it('reloads external changes through the graph owner and pauses while a local edit is pending', async () => {
  vi.useFakeTimers();
  const getNodeControls = vi.fn().mockResolvedValue(snapshot);
  const port = { getNodeControls } as unknown as ControlPort;
  const application = queue();
  const view = renderHook(({ idle }) => useNodeControls(port, 'a', 1, idle, application), {
    initialProps: { idle: false },
  });
  expect(getNodeControls).not.toHaveBeenCalled();
  view.rerender({ idle: true });
  await act(() => vi.advanceTimersByTimeAsync(1000));
  expect(application.refresh).toHaveBeenCalledTimes(1);
  expect(view.result.current).toEqual(snapshot);
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
});
it('discards a late old-graph response and does not issue overlapping polls', async () => {
  vi.useFakeTimers();
  const pending = Promise.withResolvers<GraphNodeControls>();
  const getNodeControls = vi
    .fn()
    .mockReturnValueOnce(pending.promise)
    .mockResolvedValue({ ...snapshot, graphId: 'b' });
  const port = { getNodeControls } as unknown as ControlPort;
  const application = queue();
  const view = renderHook(({ graphId }) => useNodeControls(port, graphId, 2, true, application), {
    initialProps: { graphId: 'a' },
  });
  await act(() => vi.advanceTimersByTimeAsync(2000));
  expect(getNodeControls).toHaveBeenCalledTimes(1);
  view.rerender({ graphId: 'b' });
  await act(async () => pending.resolve(snapshot));
  expect(view.result.current?.graphId).toBe('b');
  expect(application.refresh).not.toHaveBeenCalled();
  view.unmount();
});
