import { describe, expect, it, vi } from 'vitest';

import { HttpControlPort } from './HttpControlPort';
import { createLocalControlPort } from './localControlPort';
import { TauriControlPort } from './TauriControlPort';
import { ControlPortError, type RuntimeGraphDocument } from './types';

const graph: RuntimeGraphDocument = { edges: [], nodes: [], revision: 3, schemaVersion: 2 };

it.each(['device-a', 'device-b'])(
  'forwards the %s rate change without rewriting the graph document',
  async (endpointId) => {
    const invoke = vi
      .fn()
      .mockResolvedValue({ status: 202, body: { revision: 4, state: 'pending' } });
    const port = new TauriControlPort(invoke);
    const request = { endpointId, expectedRevision: 3, sampleRate: 44100 };
    await expect(port.setVirtualEndpointSampleRate(request)).resolves.toMatchObject({
      revision: 4,
    });
    expect(invoke).toHaveBeenCalledExactlyOnceWith('control_set_virtual_endpoint_sample_rate', {
      request,
    });
  },
);

describe('TauriControlPort', () => {
  it('resolves a fresh desktop filename through the interactive native session', async () => {
    const invoke = vi.fn().mockResolvedValue('/Users/test/Desktop/take.wav');
    const port = new TauriControlPort(invoke);
    await expect(port.defaultRecordingPath()).resolves.toBe('/Users/test/Desktop/take.wav');
    expect(invoke).toHaveBeenCalledExactlyOnceWith('default_recording_path', {
      fileName: expect.stringMatching(/^ToneNari-\d{4}-\d{2}-\d{2}_.*\.wav$/),
    });
    invoke.mockRejectedValueOnce('Desktop unavailable');
    await expect(port.defaultRecordingPath()).rejects.toMatchObject({
      body: { code: 'control_request_failed', parameters: { reason: 'Desktop unavailable' } },
    });
  });
  it('forwards recorder status/actions and the native WAV picker without graph writes', async () => {
    const invoke = vi.fn().mockResolvedValue({ status: 200, body: { state: 'stopped' } });
    const port = new TauriControlPort(invoke);
    await port.recorderStatus('graph-a', 'take-b');
    const request = {
      action: 'record' as const,
      path: '/take.wav',
      sampleRate: null,
      encoding: 'float32' as const,
    };
    await port.controlRecorder('graph-a', 'take-b', request);
    expect(invoke).toHaveBeenNthCalledWith(1, 'control_get_recorder_status', {
      graphId: 'graph-a',
      nodeId: 'take-b',
    });
    expect(invoke).toHaveBeenNthCalledWith(2, 'control_set_recorder', {
      graphId: 'graph-a',
      nodeId: 'take-b',
      request,
    });
    invoke.mockResolvedValueOnce('/chosen.wav');
    await expect(
      port.selectRecordingPath({ defaultPath: '/take.wav', title: 'Choose file' }),
    ).resolves.toBe('/chosen.wav');
    expect(invoke).toHaveBeenLastCalledWith('select_recording_path', {
      defaultPath: '/take.wav',
      title: 'Choose file',
    });
  });
  it('forwards layout without submitting the audio document', async () => {
    const invoke = vi.fn().mockResolvedValue({ status: 200, body: { graphId: 'graph-2' } });
    const port = new TauriControlPort(invoke);
    const request = {
      expectedLayoutRevision: 2,
      layout: { positions: { source: { x: 1, y: 2 } } },
    };
    await port.getGraphLayout('graph-2');
    await port.setGraphLayout('graph-2', request);
    expect(invoke).toHaveBeenNthCalledWith(1, 'control_get_graph_layout', { graphId: 'graph-2' });
    expect(invoke).toHaveBeenNthCalledWith(2, 'control_set_graph_layout', {
      graphId: 'graph-2',
      request,
    });
  });
});

describe('TauriControlPort graph operations', () => {
  it('forwards durable catalog requests through native control commands', async () => {
    const invoke = vi.fn().mockResolvedValue({ status: 200, body: { activeId: 'graph-2' } });
    const port = new TauriControlPort(invoke);
    await port.getGraphCatalog();
    const request = {
      action: 'select' as const,
      id: 'graph-2',
      expectedRevision: 5,
      expectedCatalogRevision: 2,
    };
    await port.mutateGraphCatalog(request);
    expect(invoke).toHaveBeenNthCalledWith(1, 'control_get_graph_catalog', undefined);
    expect(invoke).toHaveBeenNthCalledWith(2, 'control_mutate_graph_catalog', { request });
  });
  it('uses typed IPC commands for the packaged application', async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({ body: { driverState: 'ready' }, status: 200 })
      .mockResolvedValueOnce({ body: graph, status: 200 })
      .mockResolvedValueOnce({ body: { activeRevision: 3 }, status: 200 })
      .mockResolvedValueOnce({ body: { revision: 4, state: 'pending' }, status: 202 })
      .mockResolvedValueOnce({ body: { processors: [], protocolVersion: 1 }, status: 200 })
      .mockResolvedValueOnce({
        body: { nodeId: 'vst-1', parameterId: 42, protocolVersion: 1, value: 0.75 },
        status: 200,
      })
      .mockResolvedValueOnce({
        body: { failure: null, nodeId: 'vst-1', protocolVersion: 1, state: 'closed' },
        status: 200,
      })
      .mockResolvedValueOnce({
        body: { failure: null, nodeId: 'vst-1', protocolVersion: 1, state: 'opening' },
        status: 200,
      });
    const port = new TauriControlPort(invoke);

    await expect(port.getStatus()).resolves.toMatchObject({ driverState: 'ready' });
    await expect(port.getGraph()).resolves.toBe(graph);
    await expect(port.getGraphStatus()).resolves.toMatchObject({ activeRevision: 3 });
    await expect(port.applyGraph({ ...graph, revision: 4 })).resolves.toMatchObject({
      revision: 4,
    });
    await expect(port.getProcessors()).resolves.toMatchObject({ processors: [] });
    await expect(port.setProcessorParameter('vst-1', 42, 0.75)).resolves.toMatchObject({
      value: 0.75,
    });
    await expect(port.getProcessorEditor('vst-1')).resolves.toMatchObject({ state: 'closed' });
    await expect(port.setProcessorEditor('vst-1', true)).resolves.toMatchObject({
      state: 'opening',
    });
    expect(invoke).toHaveBeenNthCalledWith(1, 'control_get_status', undefined);
    expect(invoke).toHaveBeenNthCalledWith(4, 'control_apply_graph', {
      document: { ...graph, revision: 4 },
    });
    expect(invoke).toHaveBeenNthCalledWith(6, 'control_set_processor_parameter', {
      nodeId: 'vst-1',
      parameterId: 42,
      value: 0.75,
    });
    expect(invoke).toHaveBeenNthCalledWith(7, 'control_get_processor_editor', {
      nodeId: 'vst-1',
    });
    expect(invoke).toHaveBeenNthCalledWith(8, 'control_set_processor_editor', {
      nodeId: 'vst-1',
      open: true,
    });
  });

  it('preserves protocol failures without treating all IPC rejections as engine outages', async () => {
    const body = { code: 'graph_revision_stale' as const, parameters: { submittedRevision: 4 } };
    const rejected = new TauriControlPort(vi.fn().mockResolvedValue({ body, status: 409 }));
    await expect(rejected.getGraph()).rejects.toEqual(new ControlPortError(409, body));

    const invalid = new TauriControlPort(vi.fn().mockRejectedValue('missing field `id`'));
    await expect(invalid.applyGraph(graph)).rejects.toEqual(
      new ControlPortError(500, {
        code: 'control_request_failed',
        parameters: { command: 'control_apply_graph', reason: 'missing field `id`' },
      }),
    );
    const offlineBody = { code: 'control_unavailable' as const, parameters: {} };
    const offline = new TauriControlPort(
      vi.fn().mockResolvedValue({ status: 503, body: offlineBody }),
    );
    await expect(offline.getStatus()).rejects.toEqual(new ControlPortError(503, offlineBody));
  });

  it('selects HTTP for browsers and IPC for the packaged app', () => {
    expect(createLocalControlPort(false)).toBeInstanceOf(HttpControlPort);
    expect(createLocalControlPort(true)).toBeInstanceOf(TauriControlPort);
  });
});
