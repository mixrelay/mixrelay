import { expect, it, vi } from 'vitest';
import { ControlPortError, type RuntimeGraphDocument } from '../../core/control-port/types';
import { GraphApplicationQueue } from './GraphApplicationQueue';
import { audioGraphToRuntimeGraph, runtimeGraphToAudioGraph } from './model';

const document: RuntimeGraphDocument = {
  schemaVersion: 2,
  revision: 7,
  edges: [],
  nodes: [
    {
      id: 'processor',
      type: 'gain',
      label: 'Gain',
      config: { gainDb: -3 },
      control: { enabled: false, muted: true },
    },
    {
      id: 'missing',
      type: 'unknown_plugin',
      label: null,
      config: { state: 'preserved' },
      control: { enabled: false, muted: false },
    },
  ],
};

it('preserves power and mute for known and unresolved nodes through graph projection', () => {
  expect(audioGraphToRuntimeGraph(runtimeGraphToAudioGraph(document), document)).toEqual(document);
});

function fixture() {
  const restored = { ...document, revision: 8 };
  const completion = Promise.withResolvers<{
    protocolVersion: 1;
    revision: number;
    state: 'active';
  }>();
  const port = {
    updateNodeControl: vi.fn().mockReturnValue(completion.promise),
    applyGraph: vi.fn(),
    getGraph: vi.fn().mockResolvedValue(restored),
    getGraphStatus: vi
      .fn()
      .mockResolvedValue({ activeRevision: 8, submittedRevision: 8, protocolVersion: 1 }),
  };
  const callbacks = {
    getDocument: () => document,
    getModel: () => runtimeGraphToAudioGraph(document),
    document: vi.fn(),
    restore: vi.fn(),
    state: vi.fn(),
    error: vi.fn(),
  };
  return {
    completion,
    port,
    callbacks,
    queue: new GraphApplicationQueue(port, callbacks),
    restored,
  };
}

it('uses the same writer lock and displayed revision without optimistic UI state', async () => {
  const f = fixture();
  const task = f.queue.setNodeControl('graph-a', 'processor', { enabled: true });
  expect(f.port.updateNodeControl).toHaveBeenCalledExactlyOnceWith('graph-a', 'processor', {
    expectedRevision: 7,
    enabled: true,
  });
  expect(f.callbacks.restore).not.toHaveBeenCalled();
  await f.queue.setNodeControl('graph-a', 'processor', { muted: false });
  expect(f.port.updateNodeControl).toHaveBeenCalledTimes(1);
  f.completion.resolve({ revision: 8, state: 'active', protocolVersion: 1 });
  await task;
  expect(f.callbacks.restore).toHaveBeenCalledExactlyOnceWith(f.restored);
  expect(f.callbacks.state).toHaveBeenLastCalledWith('active');
});

it('keeps a rejected deleted-node command recoverable without rewriting connections', async () => {
  const f = fixture();
  const task = f.queue.setNodeControl('graph-a', 'processor', { enabled: true });
  f.completion.reject(new ControlPortError(404, { code: 'node_not_found', parameters: {} }));
  await task;
  expect(f.callbacks.restore).toHaveBeenCalledExactlyOnceWith(f.restored);
  expect(f.callbacks.error).toHaveBeenLastCalledWith('node_not_found');
  expect(f.callbacks.state).toHaveBeenLastCalledWith('active');
  expect(f.port.applyGraph).not.toHaveBeenCalled();
  f.port.updateNodeControl.mockResolvedValueOnce({
    revision: 8,
    protocolVersion: 1,
    state: 'active',
  });
  await f.queue.setNodeControl('graph-a', 'missing', { muted: true });
  expect(f.port.updateNodeControl).toHaveBeenLastCalledWith('graph-a', 'missing', {
    expectedRevision: 8,
    muted: true,
  });
  expect(f.callbacks.state).toHaveBeenLastCalledWith('active');
});

it('does not apply a late acknowledgement after the workspace is replaced', async () => {
  const f = fixture();
  const task = f.queue.setNodeControl('graph-a', 'processor', { enabled: true });
  f.queue.reset();
  f.completion.resolve({ revision: 8, state: 'active', protocolVersion: 1 });
  await task;
  expect(f.callbacks.restore).not.toHaveBeenCalled();
});

it('does not retarget queued power intent to a replacement node with the same ID', async () => {
  const f = fixture();
  let current = document;
  const pending = Promise.withResolvers<void>();
  const queue = new GraphApplicationQueue(f.port, {
    ...f.callbacks,
    beforeApply: () => pending.promise,
    getDocument: () => current,
  });
  const task = queue.setNodeControl('graph-a', 'processor', { enabled: true });
  current = { ...document, revision: 9 };
  pending.resolve();
  await task;
  expect(f.port.updateNodeControl).not.toHaveBeenCalled();
  expect(f.callbacks.error).toHaveBeenLastCalledWith('graph_revision_stale');
});

it('preserves a concurrently edited draft after a rejected node command', async () => {
  const f = fixture();
  const task = f.queue.setNodeControl('graph-a', 'processor', { enabled: true });
  f.queue.submit(runtimeGraphToAudioGraph({ ...document, revision: 9 }));
  f.completion.reject(new ControlPortError(409, { code: 'node_disabled', parameters: {} }));
  await task;
  expect(f.callbacks.restore).not.toHaveBeenCalled();
  expect(f.callbacks.state).toHaveBeenLastCalledWith('blocked');
  expect(f.port.applyGraph).not.toHaveBeenCalled();
});

it.each([true, false])(
  'uses the authoritative rejection marker (%s), not a 503 alone, to keep controls usable',
  async (definite) => {
    const f = fixture();
    const task = f.queue.setNodeControl('graph-a', 'processor', { enabled: true });
    f.completion.reject(
      new ControlPortError(503, {
        code: 'control_unavailable',
        parameters: definite ? { nodeControlOutcome: 'not_applied' } : {},
      }),
    );
    await task;
    expect(f.callbacks.state).toHaveBeenLastCalledWith(definite ? 'active' : 'blocked');
    expect(f.callbacks.restore).toHaveBeenCalledTimes(definite ? 1 : 0);
    expect(f.callbacks.error).toHaveBeenLastCalledWith('control_unavailable');
    f.port.updateNodeControl.mockResolvedValueOnce({
      revision: 8,
      protocolVersion: 1,
      state: 'active',
    });
    await f.queue.setNodeControl('graph-a', 'missing', { muted: true });
    expect(f.port.updateNodeControl).toHaveBeenCalledTimes(definite ? 2 : 1);
    if (definite)
      expect(f.port.updateNodeControl).toHaveBeenLastCalledWith('graph-a', 'missing', {
        expectedRevision: 8,
        muted: true,
      });
    expect(f.port.applyGraph).not.toHaveBeenCalled();
  },
);
