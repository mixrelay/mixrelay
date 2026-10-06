import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  ControlPortError,
  type GraphApplyResponse,
  type RuntimeGraphDocument,
} from '../../core/control-port/types';
import { GRAPH_APPLICATION_TIMEOUT_MS, GraphApplicationQueue } from './GraphApplicationQueue';
import { addRoutingNode, runtimeGraphToAudioGraph } from './model';
import { virtualSpec } from './virtualEndpointFixtures';

const empty: RuntimeGraphDocument = { schemaVersion: 2, revision: 1, nodes: [], edges: [] };
function fixture(beforeReload?: () => Promise<void>) {
  let draft = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_input', spec: virtualSpec },
    { x: 20, y: 30 },
  );
  const port = {
    getGraph: vi.fn().mockResolvedValue(empty),
    getGraphStatus: vi
      .fn()
      .mockResolvedValue({ activeRevision: 1, submittedRevision: 1, protocolVersion: 1 }),
    applyGraph: vi.fn(
      async (document: RuntimeGraphDocument): Promise<GraphApplyResponse> => ({
        protocolVersion: 1,
        revision: document.revision,
        state: 'active',
      }),
    ),
  };
  const callbacks = {
    beforeReload,
    getDocument: () => empty,
    getModel: () => draft,
    document: vi.fn(),
    restore: vi.fn(),
    state: vi.fn(),
    error: vi.fn(),
    persistenceWarning: vi.fn(),
  };
  const queue = new GraphApplicationQueue(port, callbacks);
  return {
    queue,
    port,
    callbacks,
    get draft() {
      return draft;
    },
    edit: () => {
      draft = { ...draft, revision: draft.revision + 10 };
      return draft;
    },
  };
}
const flush = () => vi.advanceTimersByTimeAsync(0);
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

it('warns about failed persistence without retrying or blocking an applied graph', async () => {
  const f = fixture();
  f.port.applyGraph.mockResolvedValueOnce({
    protocolVersion: 1,
    revision: 2,
    state: 'active',
    persistenceError: 'graph_catalog_save_failed',
  });
  f.queue.submit(f.draft);
  await flush();
  expect(f.callbacks.persistenceWarning).toHaveBeenLastCalledWith('graph_catalog_save_failed');
  expect(f.callbacks.state).toHaveBeenLastCalledWith('active');
  expect(f.callbacks.error).toHaveBeenLastCalledWith(null);
  expect(f.queue.canPublishParameters).toBe(true);
  expect(f.port.applyGraph).toHaveBeenCalledTimes(1);
  expect(f.port.getGraph).not.toHaveBeenCalled();
  expect(f.callbacks.restore).not.toHaveBeenCalled();
  f.queue.submit(f.edit());
  await flush();
  expect(f.port.applyGraph).toHaveBeenCalledTimes(2);
  expect(f.port.applyGraph.mock.calls[1]?.[0].revision).toBe(3);
  expect(f.callbacks.persistenceWarning).toHaveBeenLastCalledWith(null);
});

it('does not clear a persistence warning until a later graph is acknowledged', async () => {
  const f = fixture();
  f.port.applyGraph.mockResolvedValueOnce({
    protocolVersion: 1,
    revision: 2,
    state: 'active',
    persistenceError: 'graph_catalog_save_failed',
  });
  f.queue.submit(f.draft);
  await flush();
  f.port.applyGraph.mockRejectedValueOnce(new Error('offline'));
  f.queue.submit(f.edit());
  await flush();
  expect(f.callbacks.persistenceWarning).toHaveBeenCalledTimes(1);
  expect(f.callbacks.persistenceWarning).toHaveBeenLastCalledWith('graph_catalog_save_failed');
  expect(f.callbacks.state).toHaveBeenLastCalledWith('blocked');
});

it('serializes edits and coalesces only the queued snapshot, using engine revisions', async () => {
  const f = fixture();
  let accept!: (response: GraphApplyResponse) => void;
  f.port.applyGraph.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        accept = resolve;
      }),
  );
  f.queue.submit(f.draft);
  f.queue.submit(f.edit());
  const last = f.edit();
  f.queue.submit(last);
  expect(f.port.applyGraph).toHaveBeenCalledTimes(1);
  accept({ protocolVersion: 1, revision: 2, state: 'active' });
  await flush();
  expect(f.port.applyGraph.mock.calls.map(([doc]) => doc.revision)).toEqual([2, 3]);
  expect(f.port.applyGraph.mock.calls[1]?.[0].nodes).toHaveLength(1);
  expect(f.callbacks.state).toHaveBeenLastCalledWith('active');
  expect(vi.getTimerCount()).toBe(0);
});

it('refreshes conflicts without discarding the draft, and pauses subsequent edits', async () => {
  const f = fixture();
  f.port.applyGraph.mockRejectedValueOnce(
    new ControlPortError(409, { code: 'graph_revision_stale', parameters: {} }),
  );
  f.port.getGraph.mockResolvedValue({ ...empty, revision: 8 });
  f.queue.submit(f.draft);
  await flush();
  expect(f.callbacks.document).toHaveBeenLastCalledWith({ ...empty, revision: 8 });
  expect(f.callbacks.restore).not.toHaveBeenCalled();
  expect(f.callbacks.state).toHaveBeenLastCalledWith('blocked');
  f.queue.submit(f.edit());
  await flush();
  expect(f.port.applyGraph).toHaveBeenCalledTimes(1);
  expect(f.queue.canPublishParameters).toBe(false);
  f.port.getGraphStatus.mockResolvedValue({
    activeRevision: 8,
    submittedRevision: 8,
    protocolVersion: 1,
  });
  f.queue.retry();
  await flush();
  expect(f.port.applyGraph.mock.calls[1]?.[0].revision).toBe(9);
  expect(f.callbacks.state).toHaveBeenLastCalledWith('active');
  expect(f.queue.canPublishParameters).toBe(true);
});

it('does not treat a newer active graph as acknowledgement of our edit', async () => {
  const f = fixture();
  f.port.applyGraph.mockResolvedValue({ protocolVersion: 1, revision: 2, state: 'pending' });
  f.port.getGraphStatus.mockResolvedValue({
    activeRevision: 3,
    submittedRevision: 3,
    protocolVersion: 1,
  });
  f.queue.submit(f.draft);
  await flush();
  expect(f.callbacks.error).toHaveBeenLastCalledWith('graph_revision_stale');
  expect(f.callbacks.state).not.toHaveBeenCalledWith('active');
});

it.each(['pending', 'stalled request', 'stalled status'] as const)(
  'bounds %s and leaves no polling timers',
  async (mode) => {
    const f = fixture();
    f.port.applyGraph.mockResolvedValue({ protocolVersion: 1, revision: 2, state: 'pending' });
    if (mode === 'stalled request')
      f.port.applyGraph.mockImplementation(() => new Promise(() => {}));
    if (mode === 'stalled status')
      f.port.getGraphStatus.mockImplementation(() => new Promise(() => {}));
    f.queue.submit(f.draft);
    await vi.advanceTimersByTimeAsync(GRAPH_APPLICATION_TIMEOUT_MS + 200);
    expect(f.callbacks.error).toHaveBeenLastCalledWith('graph_apply_timeout');
    expect(f.callbacks.state).toHaveBeenLastCalledWith('blocked');
    expect(f.callbacks.restore).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  },
);

it('cancels a waiting poll and ignores a late reply after disposal', async () => {
  const f = fixture();
  let accept!: (response: GraphApplyResponse) => void;
  f.port.applyGraph.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        accept = resolve;
      }),
  );
  f.queue.submit(f.draft);
  f.queue.reset();
  accept({ protocolVersion: 1, revision: 2, state: 'active' });
  await flush();
  expect(f.callbacks.document).not.toHaveBeenCalled();
  expect(f.callbacks.state).not.toHaveBeenCalledWith('active');
  expect(vi.getTimerCount()).toBe(0);
});

it('cancels a scheduled activation poll, settling its wait', async () => {
  const f = fixture();
  f.port.applyGraph.mockResolvedValue({ protocolVersion: 1, revision: 2, state: 'pending' });
  f.queue.submit(f.draft);
  await flush();
  const polls = f.port.getGraphStatus.mock.calls.length;
  f.queue.reset();
  await vi.advanceTimersByTimeAsync(GRAPH_APPLICATION_TIMEOUT_MS * 2);
  expect(f.port.getGraphStatus).toHaveBeenCalledTimes(polls);
  expect(f.callbacks.error).toHaveBeenCalledTimes(1); // only the initial null
  expect(vi.getTimerCount()).toBe(0);
});

it('reloads the actual engine document without submitting a write', async () => {
  const f = fixture();
  f.queue.reload();
  await flush();
  expect(f.callbacks.restore).toHaveBeenCalledExactlyOnceWith(empty, undefined);
  expect(f.port.applyGraph).not.toHaveBeenCalled();
});

it('waits for parameter settlement before reading a replacement engine document', async () => {
  const settlement = Promise.withResolvers<void>();
  const f = fixture(() => settlement.promise);
  f.queue.reload();
  await flush();
  expect(f.port.getGraph).not.toHaveBeenCalled();
  expect(f.callbacks.restore).not.toHaveBeenCalled();
  settlement.resolve();
  await flush();
  expect(f.port.getGraph).toHaveBeenCalledTimes(1);
  expect(f.callbacks.restore).toHaveBeenCalledExactlyOnceWith(empty, undefined);
  expect(f.port.applyGraph).not.toHaveBeenCalled();
});

it('does not discard edits made while a reload is in flight', async () => {
  const f = fixture();
  let resolve!: (document: RuntimeGraphDocument) => void;
  f.port.getGraph.mockImplementationOnce(
    () =>
      new Promise((accept) => {
        resolve = accept;
      }),
  );
  f.queue.reload();
  f.queue.submit(f.edit());
  resolve(empty);
  await flush();
  expect(f.callbacks.restore).not.toHaveBeenCalled();
  expect(f.port.applyGraph).not.toHaveBeenCalled();
  expect(f.callbacks.error).toHaveBeenLastCalledWith('graph_draft_changed');
});
