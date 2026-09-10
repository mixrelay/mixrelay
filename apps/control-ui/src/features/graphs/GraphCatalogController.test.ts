import { afterEach, describe, expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import { ControlPortError, type GraphCatalog } from '../../core/control-port/types';
import { GraphCatalogController } from './GraphCatalogController';

function fixture() {
  const port = new StorybookMockControlPort({
    graph: { schemaVersion: 2, revision: 1, nodes: [], edges: [] },
    graphStatus: { protocolVersion: 1, activeRevision: 1, submittedRevision: 1 },
    status: {
      protocolVersion: 1,
      driverState: 'ready',
      sampleRate: 48000,
      bufferFrames: 128,
      underruns: 0,
      overruns: 0,
    },
  });
  return { port, controller: new GraphCatalogController(port) };
}

afterEach(() => vi.useRealTimers());

describe('durable graph catalog coordinator', () => {
  it('creates independent graphs and confirms activation before replacing the workspace', async () => {
    const { port, controller } = fixture();
    await controller.reload();
    const original = controller.snapshot().document;
    const mutate = vi.spyOn(port, 'mutateGraphCatalog');
    await controller.mutate({ action: 'create', name: 'Broadcast' });
    expect(mutate).toHaveBeenCalledWith({
      action: 'create',
      name: 'Broadcast',
      expectedRevision: 1,
      expectedCatalogRevision: 1,
    });
    expect(controller.snapshot().catalog?.graphs).toHaveLength(2);
    expect(controller.snapshot().document).not.toBe(original);
    expect(controller.snapshot().catalog?.activeId).toBe('graph-2');
    await controller.mutate({ action: 'select', id: 'graph-1' });
    expect(controller.snapshot().catalog?.activeId).toBe('graph-1');
  });

  it('does not reset document/history on metadata-only rename', async () => {
    const { controller } = fixture();
    await controller.reload();
    const document = controller.snapshot().document;
    const generation = controller.snapshot().documentGeneration;
    await controller.mutate({ action: 'rename', id: 'graph-1', name: 'Ableton' });
    expect(controller.snapshot().document).toBe(document);
    expect(controller.snapshot().documentGeneration).toBe(generation);
    expect(controller.snapshot().catalog?.graphs[0]?.name).toBe('Ableton');
  });

  it('retains selection/document after native rejection and preserves retryable error', async () => {
    const { port, controller } = fixture();
    await controller.reload();
    const before = controller.snapshot().catalog;
    vi.spyOn(port, 'mutateGraphCatalog').mockRejectedValue(
      new ControlPortError(422, { code: 'graph_invalid', parameters: {} }),
    );
    await controller.mutate({ action: 'create', name: 'Rejected' });
    expect(controller.snapshot().catalog).toBe(before);
    expect(controller.snapshot()).toMatchObject({
      error: 'graph_invalid',
      uncertain: false,
      busy: false,
    });
  });

  it('blocks edits after a lost mutation acknowledgement until authoritative reload', async () => {
    const { port, controller } = fixture();
    await controller.reload();
    const mutate = vi.spyOn(port, 'mutateGraphCatalog').mockRejectedValue(new Error('lost reply'));
    await controller.mutate({ action: 'create', name: 'Maybe applied' });
    expect(controller.snapshot().uncertain).toBe(true);
    await controller.mutate({ action: 'create', name: 'Must not send' });
    expect(mutate).toHaveBeenCalledTimes(1);
    await controller.reload();
    expect(controller.snapshot().uncertain).toBe(false);
  });
});

describe('catalog read recovery', () => {
  it('backs off repeated failures and caps polling at five seconds', async () => {
    vi.useFakeTimers();
    const { port, controller } = fixture();
    const read = vi.spyOn(port, 'getGraphCatalog').mockRejectedValue(new Error('offline'));
    await controller.reload();
    for (const [index, delay] of [500, 1000, 2000, 4000, 5000, 5000].entries()) {
      await vi.advanceTimersByTimeAsync(delay - 1);
      expect(read).toHaveBeenCalledTimes(index + 1);
      await vi.advanceTimersByTimeAsync(1);
      expect(read).toHaveBeenCalledTimes(index + 2);
    }
    controller.dispose();
  });

  it('recovers the initial catalog automatically after a temporary engine outage', async () => {
    vi.useFakeTimers();
    const { port, controller } = fixture();
    const read = vi
      .spyOn(port, 'getGraphCatalog')
      .mockRejectedValueOnce(
        new ControlPortError(503, { code: 'control_unavailable', parameters: {} }),
      );
    await controller.reload();
    expect(controller.snapshot().catalog).toBeNull();
    await vi.advanceTimersByTimeAsync(500);
    expect(read).toHaveBeenCalledTimes(2);
    expect(controller.snapshot().catalog?.activeId).toBe('graph-1');
    expect(controller.snapshot().error).toBeNull();
    controller.dispose();
  });

  it('does not retry a protected catalog or replay failed mutations', async () => {
    vi.useFakeTimers();
    const { port, controller } = fixture();
    const read = vi.spyOn(port, 'getGraphCatalog').mockRejectedValue(
      new ControlPortError(503, {
        code: 'control_unavailable',
        parameters: { resource: 'graph_catalog', reason: 'invalid file' },
      }),
    );
    await controller.reload();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(read).toHaveBeenCalledTimes(1);
    controller.dispose();
  });

  it('cancels scheduled recovery on disposal and never overwrites a loaded workspace', async () => {
    vi.useFakeTimers();
    const { port, controller } = fixture();
    const read = vi.spyOn(port, 'getGraphCatalog').mockRejectedValueOnce(new Error('offline'));
    await controller.reload();
    controller.dispose();
    await vi.advanceTimersByTimeAsync(5000);
    expect(read).toHaveBeenCalledTimes(1);
    await controller.reload();
    const document = controller.snapshot().document;
    read.mockRejectedValue(new Error('offline'));
    await controller.reload();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(read).toHaveBeenCalledTimes(3);
    expect(controller.snapshot().document).toBe(document);
    controller.dispose();
  });
});

describe('catalog operation lifetime', () => {
  it('distinguishes saved-graph storage failure from an engine connection failure', async () => {
    const { port, controller } = fixture();
    await controller.reload();
    vi.spyOn(port, 'mutateGraphCatalog').mockRejectedValue(
      new ControlPortError(503, {
        code: 'control_unavailable',
        parameters: { resource: 'graph_catalog', reason: 'disk unavailable' },
      }),
    );
    await controller.mutate({ action: 'create', name: 'Unsaved' });
    expect(controller.snapshot()).toMatchObject({
      error: 'graph_catalog_unavailable',
      uncertain: false,
    });
    expect(controller.snapshot().catalog?.activeId).toBe('graph-1');
  });
  it('bounds a stalled request and fences late responses after dispose', async () => {
    vi.useFakeTimers();
    const { port, controller } = fixture();
    let resolve: (catalog: GraphCatalog) => void = () => {};
    const catalog = await port.getGraphCatalog();
    vi.spyOn(port, 'getGraphCatalog').mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const pending = controller.reload();
    await vi.advanceTimersByTimeAsync(10_000);
    await pending;
    expect(controller.snapshot()).toMatchObject({ busy: false, error: 'control_request_failed' });
    controller.dispose();
    resolve(catalog);
    await Promise.resolve();
    expect(controller.snapshot().catalog).toBeNull();
  });

  it('waits for active revision and ignores late activation after disposal', async () => {
    vi.useFakeTimers();
    const { port, controller } = fixture();
    await controller.reload();
    const status = vi
      .spyOn(port, 'getGraphStatus')
      .mockResolvedValue({ protocolVersion: 1, submittedRevision: 2, activeRevision: 1 });
    const pending = controller.mutate({ action: 'create', name: 'pending' });
    await vi.advanceTimersByTimeAsync(1);
    expect(controller.snapshot().catalog?.activeId).toBe('graph-1');
    expect(controller.snapshot().busy).toBe(true);
    status.mockResolvedValue({ protocolVersion: 1, submittedRevision: 2, activeRevision: 2 });
    controller.dispose();
    await vi.advanceTimersByTimeAsync(100);
    await pending;
    expect(controller.snapshot().catalog?.activeId).toBe('graph-1');
  });
});
