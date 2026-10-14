import { describe, expect, it, vi } from 'vitest';

import { HttpControlPort } from './HttpControlPort';
import { createLocalControlPort } from './localControlPort';
import { TauriControlPort } from './TauriControlPort';
import { ControlPortError, type RuntimeGraphDocument } from './types';

const graph: RuntimeGraphDocument = { edges: [], nodes: [], revision: 3, schemaVersion: 2 };

it('sends the selected device buffer to the Driver runtime without rewriting a graph', async () => {
  const invoke = vi.fn().mockResolvedValue({ status: 200, body: { revision: 3, state: 'active' } });
  const port = new TauriControlPort(invoke);
  const request = {
    endpointId: 'device-b',
    expectedRevision: 3,
    expectedBufferFrames: null,
    expectedPreferredFrames: 512,
    bufferFrames: 256,
  };
  await expect(port.setVirtualEndpointBufferFrames(request)).resolves.toMatchObject({
    revision: 3,
  });
  expect(invoke).toHaveBeenCalledExactlyOnceWith('control_set_virtual_endpoint_buffer_frames', {
    request,
  });
});

it('forwards graph-scoped node power/mute with a captured revision', async () => {
  const invoke = vi.fn().mockResolvedValue({ status: 200, body: { revision: 4, state: 'active' } });
  const port = new TauriControlPort(invoke);
  await port.getNodeControls('graph-a');
  const request = { expectedRevision: 3, enabled: false, muted: true };
  await port.updateNodeControl('graph-a', 'node-a', request);
  expect(invoke).toHaveBeenNthCalledWith(1, 'control_get_node_controls', { graphId: 'graph-a' });
  expect(invoke).toHaveBeenNthCalledWith(2, 'control_update_node_control', {
    graphId: 'graph-a',
    nodeId: 'node-a',
    request,
  });
});

it('forwards explicit processor retry identity and both revision fences over Tauri', async () => {
  const invoke = vi.fn().mockResolvedValue({
    status: 202,
    body: { protocolVersion: 1, revision: 4, state: 'pending' },
  });
  const port = new TauriControlPort(invoke);
  const request = {
    expectedCatalogRevision: 6,
    expectedInstanceId: '18446744073709551615',
    expectedRevision: 3,
    graphId: 'graph-a',
    nodeId: 'provider-a',
  };
  await expect(port.retryProcessor(request)).resolves.toMatchObject({ revision: 4 });
  expect(invoke).toHaveBeenCalledExactlyOnceWith('control_retry_processor', { request });
});

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
  it('loads and saves canonical recorder settings without filename generation', async () => {
    const settings = {
      folder: '/Users/test/Desktop',
      sampleRate: null,
      encoding: 'pcm16' as const,
    };
    const invoke = vi.fn().mockResolvedValue(settings);
    const port = new TauriControlPort(invoke);
    await expect(port.recordingSettings('graph-a', 'take-b')).resolves.toEqual(settings);
    expect(invoke).toHaveBeenCalledExactlyOnceWith('recording_settings', {
      graphId: 'graph-a',
      nodeId: 'take-b',
    });
    await expect(port.setRecordingSettings('graph-a', 'take-b', settings, 3)).resolves.toEqual(
      settings,
    );
    expect(invoke).toHaveBeenLastCalledWith('save_recording_settings', {
      graphId: 'graph-a',
      nodeId: 'take-b',
      settings,
      expectedRevision: 3,
    });
    invoke.mockRejectedValueOnce('Desktop unavailable');
    await expect(port.recordingSettings('graph-a', 'take-b')).rejects.toMatchObject({
      body: { code: 'control_request_failed', parameters: { reason: 'Desktop unavailable' } },
    });
  });
  it('forwards action-only recorder controls and native folder selection without graph writes', async () => {
    const invoke = vi.fn().mockResolvedValue({ status: 200, body: { state: 'stopped' } });
    const port = new TauriControlPort(invoke);
    await port.recorderStatus('graph-a', 'take-b');
    const request = {
      action: 'record' as const,
    };
    await port.controlRecorder('graph-a', 'take-b', request, 3);
    expect(invoke).toHaveBeenNthCalledWith(1, 'control_get_recorder_status', {
      graphId: 'graph-a',
      nodeId: 'take-b',
    });
    expect(invoke).toHaveBeenNthCalledWith(2, 'control_set_recorder', {
      graphId: 'graph-a',
      nodeId: 'take-b',
      request: { expectedRevision: 3, request },
    });
    invoke.mockResolvedValueOnce('/chosen');
    await expect(
      port.selectRecordingFolder({ defaultPath: '/Desktop', title: 'Choose folder' }),
    ).resolves.toBe('/chosen');
    expect(invoke).toHaveBeenLastCalledWith('select_recording_folder', {
      defaultPath: '/Desktop',
      title: 'Choose folder',
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
    await expect(port.setProcessorParameter('vst-1', 42, 0.75, 3)).resolves.toMatchObject({
      value: 0.75,
    });
    await expect(port.getProcessorEditor('vst-1')).resolves.toMatchObject({ state: 'closed' });
    await expect(port.setProcessorEditor('vst-1', true, 3)).resolves.toMatchObject({
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
      expectedRevision: 3,
    });
    expect(invoke).toHaveBeenNthCalledWith(7, 'control_get_processor_editor', {
      nodeId: 'vst-1',
    });
    expect(invoke).toHaveBeenNthCalledWith(8, 'control_set_processor_editor', {
      nodeId: 'vst-1',
      open: true,
      expectedRevision: 3,
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

describe('TauriControlPort workspace and editor boundaries', () => {
  it('treats a cancelled native workspace picker as cancellation and preserves protocol failure', async () => {
    const request = {
      action: 'open' as const,
      expectedRevision: 3,
      expectedCatalogRevision: 1,
      locale: 'ko',
    };
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce({
        status: 200,
        body: { activeId: 'graph-2' },
      })
      .mockResolvedValueOnce({ status: 422, body: { code: 'graph_invalid', parameters: {} } });
    const port = new TauriControlPort(invoke);
    await expect(port.workspaceFile(request)).resolves.toBeNull();
    await expect(port.workspaceFile(request)).resolves.toEqual({ activeId: 'graph-2' });
    await expect(port.workspaceFile(request)).rejects.toMatchObject({
      status: 422,
      body: { code: 'graph_invalid' },
    });
    expect(invoke).toHaveBeenNthCalledWith(1, 'workspace_file', { request });
  });

  it('keeps explicit power, meter and reconfiguration requests independent of graph writes', async () => {
    const invoke = vi
      .fn()
      .mockResolvedValue({ status: 200, body: { revision: 3, state: 'active' } });
    const port = new TauriControlPort(invoke);
    await port.setEnginePower(false);
    await port.getMeters();
    const request = { expectedRevision: 3, format: { sampleRate: 96000, blockFrames: 128 } };
    await port.reconfigureSession(request);
    expect(invoke.mock.calls).toEqual([
      ['control_set_engine_power', { enabled: false }],
      ['control_get_meters', undefined],
      ['control_reconfigure_session', { request }],
    ]);
  });

  it('forwards native processor frames, input and rescan without graph replacement', async () => {
    const invoke = vi.fn().mockResolvedValue({ status: 200, body: { protocolVersion: 1 } });
    const port = new TauriControlPort(invoke);
    const input = {
      instanceId: 'instance-1',
      session: 3,
      kind: 'down' as const,
      x: 12,
      y: 24,
      modifiers: 0,
    };
    await port.getProcessorEditorFrame('fx-1');
    await port.sendProcessorEditorInput('fx-1', input);
    await port.rescanProcessors(['/Library/Audio/Plug-Ins/VST3']);
    expect(invoke.mock.calls).toEqual([
      ['control_get_processor_editor_frame', { nodeId: 'fx-1' }],
      ['control_send_processor_editor_input', { nodeId: 'fx-1', input }],
      ['control_rescan_processors', { paths: ['/Library/Audio/Plug-Ins/VST3'] }],
    ]);
  });
});

describe('TauriControlPort native pickers and inventory', () => {
  it('returns native application and audio device inventory and nullable directory selection', async () => {
    const applications = [{ name: 'Firefox', id: 'org.mozilla.firefox' }];
    const devices = [{ id: 'output-1', name: 'Speakers' }];
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(applications)
      .mockResolvedValueOnce(devices)
      .mockResolvedValueOnce('/plugins')
      .mockResolvedValueOnce(null);
    const port = new TauriControlPort(invoke);
    await expect(port.listCapturableApplications()).resolves.toEqual(applications);
    await expect(port.listAudioDevices()).resolves.toEqual(devices);
    await expect(port.selectProcessorDirectory()).resolves.toBe('/plugins');
    await expect(port.selectProcessorDirectory()).resolves.toBeNull();
    expect(invoke.mock.calls).toEqual([
      ['list_capturable_applications'],
      ['list_audio_devices'],
      ['select_processor_directory'],
      ['select_processor_directory'],
    ]);
  });

  it.each([
    'applications',
    'devices',
    'processor-directory',
    'recording-directory',
    'save-settings',
  ] as const)(
    'reports a native %s failure as a request failure, not an audio engine outage',
    async (operation) => {
      const cause = new Error('Native access denied');
      const port = new TauriControlPort(vi.fn().mockRejectedValue(cause));
      const result =
        operation === 'applications'
          ? port.listCapturableApplications()
          : operation === 'devices'
            ? port.listAudioDevices()
            : operation === 'processor-directory'
              ? port.selectProcessorDirectory()
              : operation === 'recording-directory'
                ? port.selectRecordingFolder({})
                : port.setRecordingSettings(
                    'graph-1',
                    'take-1',
                    { folder: '/Desktop', sampleRate: null, encoding: 'pcm16' },
                    3,
                  );
      await expect(result).rejects.toMatchObject({
        status: 500,
        body: { code: 'control_request_failed', parameters: { reason: 'Native access denied' } },
      });
    },
  );
});

describe('pushed meters', () => {
  it('relays channel messages to the listener and unsubscribes by id', async () => {
    const invoke = vi.fn().mockResolvedValue(7);
    let channel!: { onmessage: (message: unknown) => void };
    const port = new TauriControlPort(invoke, () => {
      channel = { onmessage: () => undefined };
      return channel;
    });
    const listener = { window: vi.fn(), ended: vi.fn() };
    const unsubscribe = await port.subscribeMeters(listener);
    expect(invoke).toHaveBeenCalledWith('control_subscribe_telemetry', { channel });
    const telemetry = {
      engineIncarnation: 'engine-a',
      sequence: 1,
      meters: { protocolVersion: 1, revision: 2, nodes: [] },
    };
    invoke.mockRejectedValueOnce(new Error('subscription gone'));
    channel.onmessage(telemetry);
    channel.onmessage({ ended: true });
    expect(listener.window).toHaveBeenCalledWith(telemetry);
    expect(listener.ended).toHaveBeenCalledOnce();
    // Each consumed window returns a credit; the end of the stream does not.
    expect(invoke).toHaveBeenCalledTimes(3);
    expect(invoke).toHaveBeenNthCalledWith(2, 'control_telemetry_credit', { id: 7 });
    expect(invoke).toHaveBeenLastCalledWith('control_unsubscribe_telemetry', { id: 7 });
    invoke.mockRejectedValueOnce(new Error('runtime gone'));
    unsubscribe();
    await Promise.resolve();
    expect(invoke).toHaveBeenLastCalledWith('control_unsubscribe_telemetry', { id: 7 });
    // A failed unsubscribe neither delivers nor returns credit afterwards.
    channel.onmessage(telemetry);
    expect(listener.window).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledTimes(3);
  });

  it('returns credit for windows that arrived before the subscription id', async () => {
    let subscribed!: (id: number) => void;
    const invoke = vi.fn((command: string) =>
      command === 'control_subscribe_telemetry'
        ? new Promise<number>((resolve) => (subscribed = resolve))
        : Promise.resolve(),
    );
    let channel!: { onmessage: (message: unknown) => void };
    const port = new TauriControlPort(invoke as never, () => {
      channel = { onmessage: () => undefined };
      return channel;
    });
    const listener = { window: vi.fn(), ended: vi.fn() };
    const subscribing = port.subscribeMeters(listener);
    const telemetry = {
      engineIncarnation: 'engine-a',
      sequence: 1,
      meters: { protocolVersion: 1, revision: 2, nodes: [] },
    };
    channel.onmessage(telemetry);
    channel.onmessage({ ...telemetry, sequence: 2 });
    expect(invoke).toHaveBeenCalledOnce();
    subscribed(4);
    await subscribing;
    expect(invoke.mock.calls.slice(1)).toEqual([
      ['control_telemetry_credit', { id: 4 }],
      ['control_telemetry_credit', { id: 4 }],
    ]);
  });
});

it('reads device clients with a no-argument native invoke and preserves unavailable failures', async () => {
  const activity = {
    native: [{ bundleId: 'app', displayName: 'App', processId: 1, playing: true, recording: true }],
  };
  const invoke = vi.fn().mockResolvedValue(activity);
  const port = new TauriControlPort(invoke);
  await expect(port.getVirtualDeviceAppActivity()).resolves.toEqual(activity);
  expect(invoke).toHaveBeenCalledExactlyOnceWith('list_device_app_activity');
  invoke.mockRejectedValueOnce(new Error('unsupported'));
  await expect(port.getVirtualDeviceAppActivity()).rejects.toThrow('unsupported');
  expect(new HttpControlPort()).not.toHaveProperty('getVirtualDeviceAppActivity');
});
