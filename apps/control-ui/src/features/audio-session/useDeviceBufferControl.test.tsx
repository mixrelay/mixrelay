import { act, renderHook, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import { ControlPortError, type MixRelayStatus } from '../../core/control-port/types';
import { i18n } from '../../core/i18n';
import { virtualPolicy } from '../routing-workspace/virtualEndpointFixtures';
import { useDeviceBufferControl } from './useDeviceBufferControl';

const status: MixRelayStatus = {
  protocolVersion: 1,
  driverState: 'ready',
  sampleRate: 48000,
  bufferFrames: 128,
  underruns: 0,
  overruns: 0,
  audioSession: {
    format: { sampleRate: 48000, blockFrames: 128 },
    clockAuthority: 'engine_timer',
    observedDeviceSampleRate: null,
    issue: null,
    transportLatencyFrames: null,
    capabilities: {
      backend: 'core_audio',
      supportedSampleRates: [48000],
      supportedBlockFrames: [128],
      canReconfigureLive: true,
      hardwareInput: true,
      hardwareOutput: true,
      virtualEndpoint: true,
      applicationCapture: true,
      sampleRateConversion: true,
      driftCorrection: true,
    },
    virtualEndpointSettings: {
      ...virtualPolicy,
      devices: virtualPolicy.devices.map((device) => ({
        ...device,
        spec: { ...device.spec, id: 'device-b' },
        nativeBuffer: {
          currentFrames: 512,
          preferredFrames: null,
          minimumFrames: 32,
          maximumFrames: 4096,
          canSet: true,
        },
      })),
    },
  },
};

function port() {
  return Object.assign(
    new StorybookMockControlPort({
      status,
      graph: { revision: 7, schemaVersion: 2, nodes: [], edges: [] },
      graphStatus: { protocolVersion: 1, activeRevision: 7, submittedRevision: 7 },
    }),
    {
      setVirtualEndpointBufferFrames: vi
        .fn()
        .mockResolvedValue({ protocolVersion: 1, revision: 7, state: 'active' }),
    },
  );
}

it('changes only the selected Driver device and refreshes observations without editing the graph', async () => {
  const source = port();
  const applyGraph = vi.spyOn(source, 'applyGraph');
  const busy = vi.fn();
  const applied = vi.fn();
  const hook = renderHook(() => useDeviceBufferControl(source, status, busy, applied));
  act(() => hook.result.current.change('device-b', 128));
  await waitFor(() => expect(applied).toHaveBeenCalledOnce());
  expect(source.setVirtualEndpointBufferFrames).toHaveBeenCalledExactlyOnceWith({
    endpointId: 'device-b',
    expectedRevision: 7,
    expectedBufferFrames: 512,
    expectedPreferredFrames: null,
    bufferFrames: 128,
  });
  expect(busy.mock.calls).toEqual([[true], [false]]);
  expect(applyGraph).not.toHaveBeenCalled();
  expect(status.audioSession?.format.blockFrames).toBe(128);
  expect(
    status.audioSession?.virtualEndpointSettings?.devices[0]?.nativeBuffer?.currentFrames,
  ).toBe(512);
});

it('does not send unsupported, unknown, or disabled selections', () => {
  const source = port();
  const hook = renderHook(
    ({ disabled }) => useDeviceBufferControl(source, status, vi.fn(), vi.fn(), disabled),
    {
      initialProps: { disabled: false },
    },
  );
  act(() => {
    hook.result.current.change('missing', 128);
    hook.result.current.change('device-b', 16);
    hook.result.current.change('device-b', 8192);
    hook.result.current.change('device-b', 64.5);
  });
  hook.rerender({ disabled: true });
  act(() => hook.result.current.change('device-b', 128));
  expect(source.setVirtualEndpointBufferFrames).not.toHaveBeenCalled();
  expect(hook.result.current.enabled).toBe(false);
});

it('fences saved intent for an inactive device without presenting it as an actual buffer', async () => {
  const source = port();
  const inactive: MixRelayStatus = structuredClone(status);
  const buffer = inactive.audioSession?.virtualEndpointSettings?.devices[0]?.nativeBuffer;
  if (!buffer) throw new Error('device fixture missing');
  buffer.currentFrames = null;
  buffer.preferredFrames = 256;
  const applied = vi.fn();
  const hook = renderHook(() => useDeviceBufferControl(source, inactive, vi.fn(), applied));
  act(() => hook.result.current.change('device-b', 128));
  await waitFor(() => expect(applied).toHaveBeenCalledOnce());
  expect(source.setVirtualEndpointBufferFrames).toHaveBeenCalledExactlyOnceWith({
    endpointId: 'device-b',
    expectedRevision: 7,
    expectedBufferFrames: null,
    expectedPreferredFrames: 256,
    bufferFrames: 128,
  });
  expect(buffer.currentFrames).toBeNull();
  expect(buffer.preferredFrames).toBe(256);
});

it('reports a rejected stale edit and re-reads actual Driver state', async () => {
  await i18n.changeLanguage('en');
  const source = port();
  source.setVirtualEndpointBufferFrames.mockRejectedValue(
    new ControlPortError(409, {
      code: 'graph_revision_stale',
      parameters: { resource: 'virtual_endpoint_buffer' },
    }),
  );
  const applied = vi.fn();
  const hook = renderHook(() => useDeviceBufferControl(source, status, vi.fn(), applied));
  act(() => hook.result.current.change('device-b', 128));
  await waitFor(() => expect(hook.result.current.error?.endpointId).toBe('device-b'));
  expect(applied).toHaveBeenCalledOnce();
  expect(hook.result.current.error?.message).toBe(i18n.t('routing.errors.graph_revision_stale'));
});

it('releases the old busy lease once and ignores its late completion after owner replacement', async () => {
  const old = port();
  let resolve!: (value: unknown) => void;
  old.setVirtualEndpointBufferFrames.mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const busy = vi.fn();
  const applied = vi.fn();
  const hook = renderHook(({ source }) => useDeviceBufferControl(source, status, busy, applied), {
    initialProps: { source: old },
  });
  act(() => hook.result.current.change('device-b', 128));
  await waitFor(() => expect(old.setVirtualEndpointBufferFrames).toHaveBeenCalledOnce());
  hook.rerender({ source: port() });
  await act(async () => resolve({ protocolVersion: 1, revision: 7, state: 'active' }));
  expect(applied).not.toHaveBeenCalled();
  expect(busy.mock.calls).toEqual([[true], [false]]);
  expect(hook.result.current.error).toBeNull();
});

it('does not configure a device after its graph read finishes for a replaced owner', async () => {
  const old = port();
  let finish = (_value: Awaited<ReturnType<typeof old.getGraph>>) => {};
  vi.spyOn(old, 'getGraph').mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const busy = vi.fn();
  const applied = vi.fn();
  const hook = renderHook(({ source }) => useDeviceBufferControl(source, status, busy, applied), {
    initialProps: { source: old },
  });
  act(() => hook.result.current.change('device-b', 256));
  hook.rerender({ source: port() });
  await act(async () => finish({ schemaVersion: 2, revision: 7, nodes: [], edges: [] }));
  expect(old.setVirtualEndpointBufferFrames).not.toHaveBeenCalled();
  expect(applied).not.toHaveBeenCalled();
  expect(busy.mock.calls).toEqual([[true], [false]]);
});

it('preserves applied device readback while reporting an unsaved preference', async () => {
  await i18n.changeLanguage('en');
  const source = port();
  source.setVirtualEndpointBufferFrames.mockResolvedValue({
    protocolVersion: 1,
    revision: 7,
    state: 'active',
    persistenceError: 'permission_denied',
  });
  const applied = vi.fn();
  const hook = renderHook(() => useDeviceBufferControl(source, status, vi.fn(), applied));
  act(() => hook.result.current.change('device-b', 256));
  await waitFor(() => expect(applied).toHaveBeenCalledOnce());
  expect(hook.result.current.error).toEqual({
    endpointId: 'device-b',
    message: i18n.t('audioSession.issues.endpoint_preferences_unavailable'),
  });
});

it('reports a transport failure but ignores a failure delivered after its owner has gone', async () => {
  await i18n.changeLanguage('en');
  const old = port();
  old.setVirtualEndpointBufferFrames.mockRejectedValueOnce(new Error('IPC disconnected'));
  const applied = vi.fn();
  const hook = renderHook(
    ({ source }) => useDeviceBufferControl(source, status, vi.fn(), applied),
    {
      initialProps: { source: old },
    },
  );
  act(() => hook.result.current.change('device-b', 256));
  await waitFor(() =>
    expect(hook.result.current.error?.message).toBe(i18n.t('audioSession.changeFailed')),
  );
  let fail = (_cause: unknown) => {};
  old.setVirtualEndpointBufferFrames.mockImplementationOnce(
    () =>
      new Promise((_resolve, reject) => {
        fail = reject;
      }),
  );
  act(() => hook.result.current.change('device-b', 128));
  await waitFor(() => expect(old.setVirtualEndpointBufferFrames).toHaveBeenCalledTimes(2));
  hook.rerender({ source: port() });
  await act(async () => fail(new Error('Old IPC disconnected')));
  expect(hook.result.current.error).toBeNull();
  expect(applied).toHaveBeenCalledTimes(1);
});
