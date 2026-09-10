import { expect, it } from 'vitest';
import type { RuntimeGraphDocument, VirtualEndpointSettings } from '../../core/control-port/types';
import {
  addRoutingNode,
  audioGraphToRuntimeGraph,
  presentVirtualEndpointSettings,
  runtimeGraphToAudioGraph,
  updateVirtualEndpoint,
} from './model';
import { duplexReference, virtualPolicy, virtualSpec } from './virtualEndpointFixtures';

const empty: RuntimeGraphDocument = { schemaVersion: 2, revision: 1, nodes: [], edges: [] };
const spec = virtualSpec;
const policy = virtualPolicy;
function observed(
  settings: VirtualEndpointSettings,
  value: number | null,
  canSetSampleRate = true,
): VirtualEndpointSettings {
  return {
    ...settings,
    devices: [
      {
        nativeDeviceIds: [],
        spec,
        observedSampleRate: value,
        supportedSampleRates: [44100, 48000],
        canSetSampleRate,
      },
    ],
  };
}
it.each(['negotiation', 'persistence'] as const)(
  'keeps %s warnings on their device identity only',
  (kind) => {
    const otherSpec = { ...spec, id: 'other-device' };
    let model = addRoutingNode(
      runtimeGraphToAudioGraph(empty),
      { kind: 'virtual_input', spec },
      { x: 0, y: 0 },
    );
    model = addRoutingNode(model, { kind: 'virtual_input', spec: otherSpec }, { x: 0, y: 300 });
    const device = policy.devices[0];
    if (!device) throw new Error('Missing canonical fixture');
    const presented = presentVirtualEndpointSettings(
      model,
      { ...policy, devices: [device, { ...device, spec: otherSpec }] },
      'Device sample rate',
      {
        enabled: true,
        error: { endpointId: spec.id, sampleRate: 44100, message: 'Device-specific warning', kind },
      },
    );
    expect(presented.nodes.map((node) => node.appearance.endpointControl?.rateError)).toEqual([
      'Device-specific warning',
      null,
    ]);
  },
);

it('synchronizes actual device rate and errors across both node views without editing PCM topology', () => {
  const source = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_input', spec },
    { x: 0, y: 0 },
    undefined,
    policy,
  );
  const both = addRoutingNode(
    source,
    { kind: 'virtual_output', spec },
    { x: 400, y: 0 },
    undefined,
    policy,
  );
  const before = audioGraphToRuntimeGraph(both, empty);
  const settings = observed(policy, 48000);
  const failed = presentVirtualEndpointSettings(both, settings, 'Device sample rate', {
    enabled: true,
    error: {
      endpointId: spec.id,
      sampleRate: 44100,
      message: 'Device rejected the change',
      kind: 'negotiation',
    },
  });
  expect(failed.nodes).toHaveLength(2);
  for (const node of failed.nodes) {
    expect(node.appearance.endpointControl?.observedRate?.value).toBe(48000);
    expect(node.appearance.endpointControl?.rateError).toBe('Device rejected the change');
  }
  const applied = presentVirtualEndpointSettings(
    failed,
    observed(settings, 44100),
    'Device sample rate',
    { enabled: true, error: null },
  );
  for (const node of applied.nodes) {
    expect(node.appearance.endpointControl?.observedRate?.value).toBe(44100);
    expect(node.appearance.endpointControl?.rateError).toBeNull();
  }
  expect(audioGraphToRuntimeGraph(applied, empty)).toEqual(before);
  const unavailable = presentVirtualEndpointSettings(applied, observed(settings, null));
  for (const node of unavailable.nodes)
    expect(node.appearance.endpointControl?.observedRate).toBeUndefined();
});

it.each([true, false])(
  'enables device rate editing only with native capability: %s',
  (canSetSampleRate) => {
    const model = addRoutingNode(
      runtimeGraphToAudioGraph(empty),
      { kind: 'virtual_input', spec },
      { x: 0, y: 0 },
      undefined,
      policy,
    );
    const settings = observed(policy, 48000, canSetSampleRate);
    const enabled = presentVirtualEndpointSettings(model, settings, 'Device rate', {
      enabled: true,
      error: null,
    });
    expect(enabled.nodes[0]?.appearance.endpointControl?.sampleRateOptions).toEqual(
      canSetSampleRate ? [44100, 48000] : undefined,
    );
    const blocked = presentVirtualEndpointSettings(model, settings, 'Device rate', {
      enabled: false,
      error: null,
    });
    expect(blocked.nodes[0]?.appearance.endpointControl?.sampleRateOptions).toBeUndefined();
    expect(blocked.nodes[0]?.appearance.endpointControl?.observedRate?.value).toBe(48000);
  },
);

it.each(['virtual_input', 'virtual_output'] as const)(
  'creates %s using the device template, not a graph-wide sample rate',
  (kind) => {
    const model = addRoutingNode(
      runtimeGraphToAudioGraph(empty),
      { kind, spec },
      { x: 0, y: 0 },
      undefined,
      observed(policy, null),
    );
    expect(audioGraphToRuntimeGraph(model, empty).nodes[0]?.config).toMatchObject({
      defaultSampleRate: 44100,
      supportedSampleRates: [44100, 48000],
    });
    const explicit = addRoutingNode(
      runtimeGraphToAudioGraph(empty),
      { kind, spec },
      { x: 0, y: 0 },
      undefined,
    );
    expect(audioGraphToRuntimeGraph(explicit, empty).nodes[0]?.config).toMatchObject({
      defaultSampleRate: 44100,
      supportedSampleRates: [44100, 48000],
    });
  },
);

it.each([null, 0, Number.NaN, 48000])(
  'shows only observed own-device rate %s and never writes it into the graph',
  (observedSampleRate) => {
    const model = addRoutingNode(
      runtimeGraphToAudioGraph(empty),
      { kind: 'virtual_input', spec },
      { x: 0, y: 0 },
      undefined,
      policy,
    );
    const before = audioGraphToRuntimeGraph(model, empty);
    const presented = presentVirtualEndpointSettings(
      model,
      observed(policy, observedSampleRate),
      '장치 샘플레이트',
    );
    expect(presented.nodes[0]?.appearance.endpointControl?.observedRate).toEqual(
      observedSampleRate === 48000 ? { label: '장치 샘플레이트', value: 48000 } : undefined,
    );
    expect(audioGraphToRuntimeGraph(presented, empty)).toEqual(before);
    const missing = presentVirtualEndpointSettings(presented, observed(policy, null));
    expect(missing.nodes[0]?.appearance.endpointControl?.observedRate).toBeUndefined();
    const other = presentVirtualEndpointSettings(model, {
      ...policy,
      devices: [
        {
          nativeDeviceIds: [],
          spec: { ...spec, id: 'another-device' },
          observedSampleRate: 48000,
          canSetSampleRate: true,
          supportedSampleRates: [48000],
        },
      ],
    });
    expect(other.nodes[0]?.appearance.endpointControl?.observedRate).toBeUndefined();
  },
);

it.each(Array.from({ length: 64 }, (_, index) => index + 1))(
  'applies every advertised channel count, including odd counts: %i',
  (channels) => {
    const continuous = {
      ...policy,
      renderChannels: Array.from({ length: 64 }, (_, index) => index + 1),
      captureChannels: Array.from({ length: 64 }, (_, index) => index + 1),
    };
    const document: RuntimeGraphDocument = {
      ...empty,
      nodes: [
        {
          id: 'virtual-1',
          label: null,
          type: 'virtual_endpoint',
          config: { ...spec, reference: duplexReference },
        },
      ],
    };
    const model = runtimeGraphToAudioGraph(document);
    const changed = updateVirtualEndpoint(
      model,
      'virtual-1::host-source',
      {
        displayName: 'MixRelay IO',
        renderChannels: channels,
        captureChannels: 65 - channels,
      },
      undefined,
      continuous,
    );
    const serialized = audioGraphToRuntimeGraph(changed, document);
    expect(serialized.nodes[0]?.config).toMatchObject({
      render: { channels },
      capture: { channels: 65 - channels },
    });
    const reloaded = runtimeGraphToAudioGraph(serialized);
    expect(reloaded.nodes.find((node) => node.outputPorts.length)?.outputPorts).toHaveLength(
      Math.max(channels, 65 - channels),
    );
    expect(reloaded.nodes.find((node) => node.inputPorts.length)?.inputPorts).toHaveLength(
      65 - channels,
    );
  },
);

it.each([0, -1, 65, 7.5, Number.NaN, Number.POSITIVE_INFINITY])(
  'does not silently round an invalid channel count: %s',
  (channels) => {
    const attached = addRoutingNode(
      runtimeGraphToAudioGraph(empty),
      { kind: 'virtual_input', spec },
      { x: 0, y: 0 },
      undefined,
      policy,
    );
    expect(
      updateVirtualEndpoint(attached, 'virtual-1::host-source', {
        displayName: 'MixRelay IO',
        renderChannels: channels,
        captureChannels: 2,
      }),
    ).toBe(attached);
  },
);

it.each(['virtual_input', 'virtual_output'] as const)(
  'restores authoritative identity, name, rate and asymmetric channels: %s',
  (kind) => {
    const first = addRoutingNode(
      runtimeGraphToAudioGraph(empty),
      { kind, spec },
      { x: 0, y: 0 },
      undefined,
      policy,
    );
    const serialized = audioGraphToRuntimeGraph(first, empty);
    const both = addRoutingNode(
      runtimeGraphToAudioGraph(serialized),
      {
        kind: kind === 'virtual_input' ? 'virtual_output' : 'virtual_input',
        spec,
      },
      { x: 400, y: 0 },
      undefined,
      policy,
    );
    const restored = audioGraphToRuntimeGraph(both, serialized);
    expect(restored.nodes).toHaveLength(1);
    expect(restored.nodes[0]).toMatchObject({
      id: 'virtual-1',
      config: spec,
    });
    expect(both.nodes.find((node) => node.outputPorts.length)?.outputPorts).toHaveLength(64);
    expect(both.nodes.find((node) => node.inputPorts.length)?.inputPorts).toHaveLength(16);
    expect(addRoutingNode(both, { kind, spec }, { x: 0, y: 0 }, undefined, policy)).toBe(both);
  },
);

it('refuses unsupported creation and settings mutations, not just disabled UI clicks', () => {
  const model = runtimeGraphToAudioGraph(empty);
  const fixed = {
    ...policy,
    canCreate: false,
    devices: [],
    canRename: false,
    renderChannels: [64],
    captureChannels: [16],
  };
  expect(
    addRoutingNode(model, { kind: 'virtual_input', spec }, { x: 0, y: 0 }, undefined, fixed),
  ).toBe(model);
  expect(
    addRoutingNode(model, { kind: 'virtual_output', spec }, { x: 0, y: 0 }, undefined, {
      ...policy,
      canCreate: false,
      devices: [],
      creationTemplate: null,
    }),
  ).toBe(model);
  const attached = addRoutingNode(
    model,
    { kind: 'virtual_input', spec },
    { x: 0, y: 0 },
    undefined,
    policy,
  );
  const update = { displayName: 'new name', renderChannels: 64, captureChannels: 16 };
  expect(updateVirtualEndpoint(attached, 'virtual-1::host-source', update, undefined, fixed)).toBe(
    attached,
  );
  expect(
    updateVirtualEndpoint(
      attached,
      'virtual-1::host-source',
      { ...update, renderChannels: 2 },
      undefined,
      fixed,
    ),
  ).toBe(attached);
  const changed = updateVirtualEndpoint(
    attached,
    'virtual-1::host-source',
    { ...update, renderChannels: 8 },
    undefined,
    policy,
  );
  expect(changed.nodes[0]?.appearance.title).toBe('new name');
  expect(changed.nodes[0]?.outputPorts).toHaveLength(16);
});

it('settings presentation preserves runtime document and revision', () => {
  const model = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_input', spec },
    { x: 0, y: 0 },
    undefined,
    policy,
  );
  const presented = presentVirtualEndpointSettings(model, { ...policy, canRename: false });
  expect(presented.nodes[0]?.appearance.endpointControl).toMatchObject({
    canRename: false,
    allowedRenderChannels: policy.renderChannels,
  });
  expect(presented.revision).toBe(model.revision);
  expect(audioGraphToRuntimeGraph(presented, empty)).toEqual(
    audioGraphToRuntimeGraph(model, empty),
  );
  expect(model.nodes[0]?.appearance.endpointControl?.canRename).toBeUndefined();
});

it('renames a reloaded half-duplex attachment without inventing the absent side', () => {
  const attached = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_input', spec },
    { x: 0, y: 0 },
    undefined,
    policy,
  );
  const document = audioGraphToRuntimeGraph(attached, empty);
  const reloaded = runtimeGraphToAudioGraph(document);
  const control = presentVirtualEndpointSettings(reloaded, policy).nodes[0]?.appearance
    .endpointControl;
  expect(control?.captureChannels).toBe(16);
  expect(control?.allowedCaptureChannels).toEqual(policy.captureChannels);
  const renamed = updateVirtualEndpoint(
    reloaded,
    'virtual-1::host-source',
    {
      displayName: 'Renamed send',
      renderChannels: 64,
      captureChannels: control?.captureChannels ?? 0,
    },
    undefined,
    policy,
  );
  const saved = audioGraphToRuntimeGraph(renamed, document);
  expect(saved.nodes[0]?.config).toMatchObject({
    displayName: 'Renamed send',
    render: { channels: 64 },
  });
  expect(saved.nodes[0]?.config).toMatchObject({
    capture: spec.capture,
    reference: { render: true, capture: false },
  });
});

it('keeps valid cables when reducing a configurable endpoint channel count', () => {
  const source = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_input', spec },
    { x: 0, y: 0 },
    undefined,
    policy,
  );
  const both = addRoutingNode(
    source,
    { kind: 'virtual_output', spec },
    { x: 400, y: 0 },
    undefined,
    policy,
  );
  const output = both.nodes.find((node) => node.outputPorts.length);
  const input = both.nodes.find((node) => node.inputPorts.length);
  if (!output || !input) throw new Error('fixture requires both directions');
  const edges = [0, 63].map((channel) => ({
    id: `cable-${channel}`,
    sourceNodeId: output.id,
    sourcePortId: output.outputPorts[channel]?.id ?? '',
    targetNodeId: input.id,
    targetPortId: input.inputPorts[0]?.id ?? '',
  }));
  const updated = updateVirtualEndpoint(
    { ...both, edges },
    output.id,
    {
      displayName: 'SPAT send-return',
      renderChannels: 16,
      captureChannels: 16,
    },
    undefined,
    policy,
  );
  expect(updated.edges).toEqual([edges[0]]);
  expect(updated.nodes.find((node) => node.id === input.id)?.inputPorts).toHaveLength(16);
});
