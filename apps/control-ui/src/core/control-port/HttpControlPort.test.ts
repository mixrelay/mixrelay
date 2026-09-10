import { describe, expect, it, vi } from 'vitest';

import { HttpControlPort } from './HttpControlPort';
import { ControlPortError, type RuntimeGraphDocument } from './types';

const graph: RuntimeGraphDocument = { edges: [], nodes: [], revision: 2, schemaVersion: 2 };

it.each(['device-a', 'device-b'])(
  'sets the rate of %s through its identity-targeted route',
  async (endpointId) => {
    const request = vi
      .fn<typeof fetch>()
      .mockResolvedValue(response({ revision: 3, state: 'pending' }));
    const port = new HttpControlPort('http://control', request);
    const format = { endpointId, expectedRevision: 2, sampleRate: 44100 };
    await expect(port.setVirtualEndpointSampleRate(format)).resolves.toMatchObject({ revision: 3 });
    expect(request).toHaveBeenCalledExactlyOnceWith(
      'http://control/api/v1/audio/virtual-endpoint/format',
      {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(format),
      },
    );
  },
);

function response(body: unknown, ok = true, status = 200): Response {
  return { json: async () => body, ok, status } as Response;
}

describe('HttpControlPort', () => {
  it('targets recorder commands by encoded graph and node identity', async () => {
    const status = {
      state: 'paused',
      path: '/take.wav',
      sampleRate: 48000,
      encoding: 'pcm24',
      frames: 2048,
      error: null,
    };
    const request = vi.fn<typeof fetch>().mockResolvedValue(response(status));
    const port = new HttpControlPort('http://control', request);
    await expect(port.recorderStatus('graph/a', 'record/b')).resolves.toEqual(status);
    await expect(
      port.controlRecorder('graph/a', 'record/b', { action: 'record' }, 3),
    ).resolves.toEqual(status);
    expect(request).toHaveBeenNthCalledWith(
      1,
      'http://control/api/v1/graphs/graph%2Fa/recorders/record%2Fb',
      undefined,
    );
    expect(request).toHaveBeenNthCalledWith(
      2,
      'http://control/api/v1/graphs/graph%2Fa/recorders/record%2Fb',
      {
        method: 'PUT',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ expectedRevision: 3, request: { action: 'record' } }),
      },
    );
  });
  it('does not bind browser fetch to the adapter instance', async () => {
    const request = vi.fn(function (this: unknown) {
      expect(this).toBeUndefined();
      return Promise.resolve(response({ driverState: 'ready' }));
    });
    await expect(new HttpControlPort('http://control', request).getStatus()).resolves.toEqual({
      driverState: 'ready',
    });
  });
  it('stores presentation independently with an encoded graph ID and layout revision', async () => {
    const request = vi.fn<typeof fetch>().mockResolvedValue(response({ graphId: 'graph/1' }));
    const port = new HttpControlPort('http://control', request);
    const layout = {
      expectedLayoutRevision: 4,
      layout: { positions: {}, viewport: { x: 12, y: -3, zoom: 0.5 } },
    };
    await port.getGraphLayout('graph/1');
    await port.setGraphLayout('graph/1', layout);
    expect(request).toHaveBeenNthCalledWith(
      1,
      'http://control/api/v1/graphs/graph%2F1/layout',
      undefined,
    );
    expect(request).toHaveBeenNthCalledWith(2, 'http://control/api/v1/graphs/graph%2F1/layout', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(layout),
    });
  });
});

describe('HttpControlPort graph operations', () => {
  it('uses durable catalog routes with both concurrency baselines', async () => {
    const request = vi.fn<typeof fetch>().mockResolvedValue(response({ activeId: 'graph-1' }));
    const port = new HttpControlPort('http://control', request);
    await port.getGraphCatalog();
    const mutation = {
      action: 'create' as const,
      name: 'Broadcast',
      expectedRevision: 2,
      expectedCatalogRevision: 1,
    };
    await port.mutateGraphCatalog(mutation);
    expect(request).toHaveBeenNthCalledWith(1, 'http://control/api/v1/graphs', undefined);
    expect(request).toHaveBeenNthCalledWith(2, 'http://control/api/v1/graphs', {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(mutation),
    });
  });
  it('uses the versioned loopback API for status, graph reads, and graph writes', async () => {
    const request = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(response({ driverState: 'ready' }))
      .mockResolvedValueOnce(response(graph))
      .mockResolvedValueOnce(response({ activeRevision: 2 }))
      .mockResolvedValueOnce(response({ revision: 3, state: 'pending' }, true, 202))
      .mockResolvedValueOnce(response({ processors: [], protocolVersion: 1 }))
      .mockResolvedValueOnce(
        response({ nodeId: 'vst/node', parameterId: 42, protocolVersion: 1, value: 0.75 }),
      )
      .mockResolvedValueOnce(
        response({ failure: null, nodeId: 'vst/node', protocolVersion: 1, state: 'closed' }),
      )
      .mockResolvedValueOnce(
        response({ failure: null, nodeId: 'vst/node', protocolVersion: 1, state: 'opening' }),
      );
    const port = new HttpControlPort('http://control', request);

    await expect(port.getStatus()).resolves.toMatchObject({ driverState: 'ready' });
    await expect(port.getGraph()).resolves.toBe(graph);
    await expect(port.getGraphStatus()).resolves.toMatchObject({ activeRevision: 2 });
    await expect(port.applyGraph({ ...graph, revision: 3 })).resolves.toMatchObject({
      revision: 3,
    });
    await expect(port.getProcessors()).resolves.toMatchObject({ processors: [] });
    await expect(port.setProcessorParameter('vst/node', 42, 0.75, 3)).resolves.toMatchObject({
      value: 0.75,
    });
    await expect(port.getProcessorEditor('vst/node')).resolves.toMatchObject({ state: 'closed' });
    await expect(port.setProcessorEditor('vst/node', true, 3)).resolves.toMatchObject({
      state: 'opening',
    });
    expect(request).toHaveBeenNthCalledWith(1, 'http://control/api/v1/status', undefined);
    expect(request).toHaveBeenNthCalledWith(4, 'http://control/api/v1/graph', {
      body: JSON.stringify({ ...graph, revision: 3 }),
      headers: { 'content-type': 'application/json' },
      method: 'PUT',
    });
    expect(request).toHaveBeenNthCalledWith(
      6,
      'http://control/api/v1/processors/vst%2Fnode/parameters/42',
      {
        body: JSON.stringify({ value: 0.75, expectedRevision: 3 }),
        headers: { 'content-type': 'application/json' },
        method: 'PUT',
      },
    );
    expect(request).toHaveBeenNthCalledWith(
      7,
      'http://control/api/v1/processors/vst%2Fnode/editor',
      undefined,
    );
    expect(request).toHaveBeenNthCalledWith(
      8,
      'http://control/api/v1/processors/vst%2Fnode/editor',
      {
        body: JSON.stringify({ open: true, expectedRevision: 3 }),
        headers: { 'content-type': 'application/json' },
        method: 'PUT',
      },
    );
  });

  it('throws the stable protocol error returned by the engine', async () => {
    const body = { code: 'graph_invalid' as const, parameters: { issues: [] } };
    const port = new HttpControlPort(
      'http://control',
      vi.fn().mockResolvedValue(response(body, false, 422)),
    );

    await expect(port.getGraph()).rejects.toEqual(new ControlPortError(422, body));
  });
});
