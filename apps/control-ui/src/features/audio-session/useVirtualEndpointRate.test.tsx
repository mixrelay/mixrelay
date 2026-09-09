import { act, renderHook, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import {
  type ControlPort,
  ControlPortError,
  type VirtualEndpointSettings,
} from '../../core/control-port/types';
import { virtualPolicy, virtualSpec } from '../routing-workspace/virtualEndpointFixtures';
import { useVirtualEndpointRate } from './useVirtualEndpointRate';

const settings: VirtualEndpointSettings = {
  ...virtualPolicy,
  devices: ['device-a', 'device-b'].map((id) => ({
    nativeDeviceIds: [],
    spec: { ...virtualSpec, id, defaultSampleRate: 48000 },
    canSetSampleRate: true,
    observedSampleRate: 48000,
    supportedSampleRates: [44100, 48000],
  })),
};
const document = { schemaVersion: 2 as const, revision: 4, nodes: [], edges: [] };
function fixture() {
  const getGraph = vi.fn().mockResolvedValue(document);
  const setter = vi.fn().mockResolvedValue({ protocolVersion: 1, revision: 5, state: 'pending' });
  const getGraphCatalog = vi.fn().mockResolvedValue({ document, persistenceError: undefined });
  const port = {
    getGraph,
    getGraphCatalog,
    setVirtualEndpointSampleRate: setter,
    getGraphStatus: vi.fn().mockResolvedValue({ activeRevision: 5 }),
  } as unknown as ControlPort;
  const onApplied = vi.fn();
  const onBusyChange = vi.fn();
  return { port, getGraph, getGraphCatalog, setter, onApplied, onBusyChange };
}

it('keeps different device identities distinct and blocks missing or blank identities', async () => {
  const f = fixture();
  const { result } = renderHook(() =>
    useVirtualEndpointRate({
      controlPort: f.port,
      settings,
      enabled: true,
      onApplied: f.onApplied,
      onBusyChange: f.onBusyChange,
    }),
  );
  await act(async () => {
    // An untyped caller cannot restore the removed ID-less request path.
    // @ts-expect-error endpoint identity is mandatory
    await result.current.change(undefined, 44100);
    await result.current.change('', 44100);
    await result.current.change('  ', 44100);
  });
  expect(f.getGraph).not.toHaveBeenCalled();
  expect(f.setter).not.toHaveBeenCalled();
  expect(f.onBusyChange).not.toHaveBeenCalled();
  await act(async () => {
    await result.current.change('device-a', 44100);
    await result.current.change('device-b', 44100);
  });
  expect(f.setter.mock.calls).toEqual([
    [{ endpointId: 'device-a', expectedRevision: 4, sampleRate: 44100 }],
    [{ endpointId: 'device-b', expectedRevision: 4, sampleRate: 44100 }],
  ]);
});

it('uses the dedicated rate API with the current revision, then reloads after activation', async () => {
  const f = fixture();
  f.getGraph.mockResolvedValueOnce(document).mockResolvedValueOnce({ ...document, revision: 5 });
  const { result } = renderHook(() =>
    useVirtualEndpointRate({
      controlPort: f.port,
      settings,
      enabled: true,
      onApplied: f.onApplied,
      onBusyChange: f.onBusyChange,
    }),
  );
  await act(async () => {
    await result.current.change('device-a', 44100);
  });
  expect(f.setter).toHaveBeenCalledWith({
    endpointId: 'device-a',
    expectedRevision: 4,
    sampleRate: 44100,
  });
  expect(f.setter).toHaveBeenCalledTimes(1);
  expect(f.onApplied).toHaveBeenCalledWith({ ...document, revision: 5 });
  expect(f.onBusyChange.mock.calls).toEqual([[true], [false]]);
});

it('adopts an applied device change but preserves a persistence warning until the next success', async () => {
  const f = fixture();
  f.setter.mockResolvedValueOnce({
    protocolVersion: 1,
    revision: 5,
    state: 'active',
    persistenceError: 'disk_full',
  });
  const { result } = renderHook(() =>
    useVirtualEndpointRate({
      controlPort: f.port,
      settings,
      enabled: true,
      onApplied: f.onApplied,
      onBusyChange: f.onBusyChange,
    }),
  );
  await act(async () => {
    await result.current.change('device-a', 44100);
  });
  expect(f.onApplied).toHaveBeenCalledTimes(1);
  expect(result.current.error).toBeTruthy();
  expect(f.setter).toHaveBeenCalledTimes(1);
  await act(async () => {
    await result.current.change('device-a', 44100);
  });
  expect(result.current.error).toBeNull();
  expect(f.onApplied).toHaveBeenCalledTimes(2);
});

it.each([
  { enabled: false, policy: settings, rate: 44100 },
  {
    enabled: true,
    policy: {
      ...settings,
      devices: settings.devices.map((device) => ({ ...device, canSetSampleRate: false })),
    },
    rate: 44100,
  },
  {
    enabled: true,
    policy: {
      ...settings,
      devices: settings.devices.map((device) => ({ ...device, observedSampleRate: null })),
    },
    rate: 44100,
  },
  { enabled: true, policy: settings, rate: 96000 },
  { enabled: true, policy: settings, rate: 48000 },
])(
  'does not mutate unsupported, unknown, identical or unsaved-draft state %#',
  async ({ enabled, policy, rate }) => {
    const f = fixture();
    const { result } = renderHook(() =>
      useVirtualEndpointRate({
        controlPort: f.port,
        settings: policy,
        enabled,
        onApplied: f.onApplied,
        onBusyChange: f.onBusyChange,
      }),
    );
    await act(async () => {
      await result.current.change('device-a', rate);
    });
    expect(f.getGraph).not.toHaveBeenCalled();
    expect(f.setter).not.toHaveBeenCalled();
  },
);

it('coalesces duplicate clicks and ignores an unmounted native reply', async () => {
  const f = fixture();
  let resolve!: (value: unknown) => void;
  f.setter.mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const { result, unmount } = renderHook(() =>
    useVirtualEndpointRate({
      controlPort: f.port,
      settings,
      enabled: true,
      onApplied: f.onApplied,
      onBusyChange: f.onBusyChange,
    }),
  );
  await act(async () => {
    void result.current.change('device-a', 44100);
    void result.current.change('device-a', 44100);
  });
  expect(f.setter).toHaveBeenCalledTimes(1);
  unmount();
  await act(async () => {
    resolve({ protocolVersion: 1, revision: 5, state: 'pending' });
  });
  expect(f.getGraph).toHaveBeenCalledTimes(1);
  expect(f.onApplied).not.toHaveBeenCalled();
  expect(f.onBusyChange.mock.calls).toEqual([[true], [false]]);
});

it('rejects stale revision without replacing edits and allows an explicit retry', async () => {
  const f = fixture();
  f.setter.mockRejectedValueOnce(
    new ControlPortError(409, { code: 'graph_revision_stale', parameters: {} }),
  );
  const { result } = renderHook(() =>
    useVirtualEndpointRate({
      controlPort: f.port,
      settings,
      enabled: true,
      onApplied: f.onApplied,
      onBusyChange: f.onBusyChange,
    }),
  );
  await act(async () => {
    await result.current.change('device-a', 44100);
  });
  expect(result.current.error).toBeTruthy();
  expect(f.onApplied).not.toHaveBeenCalled();
  await act(async () => {
    await result.current.change('device-a', 44100);
  });
  expect(result.current.error).toBeNull();
  expect(f.onApplied).toHaveBeenCalledTimes(1);
});

it('invalidates an old connection before sending its request and releases only its busy lease', async () => {
  const first = fixture();
  const second = fixture();
  let resolve!: (value: unknown) => void;
  first.getGraph.mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const { result, rerender } = renderHook(
    ({ controlPort }) =>
      useVirtualEndpointRate({
        controlPort,
        settings,
        enabled: true,
        onApplied: first.onApplied,
        onBusyChange: first.onBusyChange,
      }),
    { initialProps: { controlPort: first.port } },
  );
  await act(async () => {
    void result.current.change('device-a', 44100);
  });
  rerender({ controlPort: second.port });
  await act(async () => {
    resolve(document);
  });
  expect(first.setter).not.toHaveBeenCalled();
  expect(first.onApplied).not.toHaveBeenCalled();
  expect(first.onBusyChange.mock.calls).toEqual([[true], [false]]);
  await act(async () => {
    await result.current.change('device-a', 44100);
  });
  expect(second.setter).toHaveBeenCalledTimes(1);
});

it('scopes a failed request to its device and clears it only after native and catalog reconciliation', async () => {
  const f = fixture();
  f.setter.mockRejectedValueOnce(
    new ControlPortError(500, {
      code: 'audio_session_reconfigure_failed',
      parameters: {},
    }),
  );
  const { result, rerender } = renderHook(
    ({ policy }) =>
      useVirtualEndpointRate({
        controlPort: f.port,
        settings: policy,
        enabled: true,
        onApplied: f.onApplied,
        onBusyChange: f.onBusyChange,
      }),
    { initialProps: { policy: settings } },
  );
  await act(async () => {
    await result.current.change('device-a', 44100);
  });
  expect(result.current.error).toMatchObject({
    endpointId: 'device-a',
    sampleRate: 44100,
    kind: 'negotiation',
  });
  const observed = {
    ...settings,
    devices: settings.devices.map((device) => ({
      ...device,
      observedSampleRate: 44100,
    })),
  };
  rerender({ policy: observed });
  expect(result.current.error).not.toBeNull();
  rerender({
    policy: {
      ...observed,
      devices: observed.devices.map((device) => ({
        ...device,
        spec: { ...device.spec, defaultSampleRate: 44100 },
      })),
    },
  });
  await waitFor(() => expect(result.current.error).toBeNull());
  // Later external changes must not resurrect the obsolete failure.
  rerender({ policy: settings });
  expect(result.current.error).toBeNull();
  expect(f.setter).toHaveBeenCalledTimes(1);
});

it('does not hide a save failure when the device reaches the requested rate', async () => {
  const f = fixture();
  f.setter.mockResolvedValueOnce({
    protocolVersion: 1,
    revision: 5,
    state: 'active',
    persistenceError: 'disk_full',
  });
  const { result, rerender } = renderHook(
    ({ policy }) =>
      useVirtualEndpointRate({
        controlPort: f.port,
        settings: policy,
        enabled: true,
        onApplied: f.onApplied,
        onBusyChange: f.onBusyChange,
      }),
    { initialProps: { policy: settings } },
  );
  await act(async () => {
    await result.current.change('device-a', 44100);
  });
  rerender({
    policy: {
      ...settings,
      devices: settings.devices.map((device) => ({
        ...device,
        observedSampleRate: 44100,
        spec: { ...device.spec, defaultSampleRate: 44100 },
      })),
    },
  });
  expect(result.current.error).toMatchObject({ endpointId: 'device-a', kind: 'persistence' });
});

it('recovers a lost reply without hiding the catalog durability failure', async () => {
  const f = fixture();
  f.setter.mockRejectedValueOnce(new Error('response lost'));
  f.getGraphCatalog.mockResolvedValue({ document, persistenceError: 'disk_full' });
  const { result, rerender } = renderHook(
    ({ policy }) =>
      useVirtualEndpointRate({
        controlPort: f.port,
        settings: policy,
        enabled: true,
        onApplied: f.onApplied,
        onBusyChange: f.onBusyChange,
      }),
    { initialProps: { policy: settings } },
  );
  await act(async () => {
    await result.current.change('device-a', 44100);
  });
  rerender({
    policy: {
      ...settings,
      devices: settings.devices.map((device) => ({
        ...device,
        observedSampleRate: 44100,
        spec: { ...device.spec, defaultSampleRate: 44100 },
      })),
    },
  });
  await waitFor(() =>
    expect(result.current.error).toMatchObject({
      endpointId: 'device-a',
      kind: 'persistence',
    }),
  );
  expect(f.getGraphCatalog).toHaveBeenCalledTimes(1);
});
