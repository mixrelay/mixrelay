import { expect, it } from 'vitest';
import type { AudioGraphModel, AudioGraphNode } from '../../core/audio-graph/model';
import type {
  ExternalProcessorDescriptor,
  ProviderPorts,
  RuntimeGraphDocument,
  RuntimeGraphNode,
} from '../../core/control-port/types';
import {
  addRoutingNode,
  audioGraphToRuntimeGraph,
  changedProcessorParameters,
  presentRoutingApplicationIcons,
  presentVirtualEndpointSettings,
  providerPortsFrom,
  runtimeGraphToAudioGraph,
  updateRecorderChannels,
  updateRoutingNodeGain,
  updateRoutingNodeParameter,
  updateVirtualEndpoint,
} from './model';
import { virtualPolicy, virtualSpec } from './virtualEndpointFixtures';

const empty: RuntimeGraphDocument = { schemaVersion: 2, revision: 1, nodes: [], edges: [] };
const position = { x: 40, y: 60 };
const ports: ProviderPorts = {
  layoutId: 'stereo',
  inputs: [{ id: 'input', name: 'Input', kind: 'main', channels: 2, channelFormat: 'stereo' }],
  outputs: [{ id: 'output', name: 'Output', kind: 'main', channels: 2, channelFormat: 'stereo' }],
};
const processor: ExternalProcessorDescriptor = {
  displayName: 'Effect',
  typeId: 'effect',
  format: 'vst3',
  hasEditor: false,
  layouts: [ports],
  defaultLayoutId: ports.layoutId,
  parameters: [],
  vendor: 'Fixture',
  version: '1',
};
function documentWith(...nodes: RuntimeGraphNode[]): RuntimeGraphDocument {
  return { ...empty, nodes };
}
function node(model: AudioGraphModel): AudioGraphNode {
  const first = model.nodes[0];
  if (!first) throw new Error('Expected a materialized fixture node');
  return first;
}

it.each(['limiter', 'compressor'] as const)(
  'creates and round-trips the built-in %s without external-plugin metadata',
  (kind) => {
    const created = addRoutingNode(runtimeGraphToAudioGraph(empty), { kind }, position);
    const first = node(created);
    const parameter = kind === 'limiter' ? 'ceilingDb' : 'thresholdDb';
    const changed = updateRoutingNodeParameter(created, first.id, parameter, -9.2);
    const serialized = audioGraphToRuntimeGraph(changed, empty);
    expect(serialized.nodes[0]?.config).toMatchObject({ [parameter]: -9, channels: 2 });
    const restored = runtimeGraphToAudioGraph(serialized);
    expect(
      node(restored).appearance.parameterControls?.find((control) => control.id === parameter),
    ).toMatchObject({ value: -9, suffix: 'dB' });
    expect(node(restored).runtime?.type).toBe(kind);
    expect(node(restored).appearance.detail).toContain('-9.0 dB');
    expect(updateRoutingNodeParameter(changed, first.id, parameter, -9)).toBe(changed);
  },
);

it.each([Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY])(
  'does not persist a non-finite gain or processor parameter: %s',
  (value) => {
    const model = addRoutingNode(runtimeGraphToAudioGraph(empty), { kind: 'limiter' }, position);
    expect(updateRoutingNodeGain(model, node(model).id, value)).toBe(model);
    expect(updateRoutingNodeParameter(model, node(model).id, 'ceilingDb', value)).toBe(model);
  },
);

it('clamps gain at the supported bounds and keeps unchanged commands idempotent', () => {
  const model = runtimeGraphToAudioGraph(
    documentWith({
      id: 'gain',
      type: 'gain',
      label: null,
      config: { gainDb: 0, channels: 2 },
    }),
  );
  const limited = updateRoutingNodeGain(model, 'gain', 100);
  expect(node(limited).appearance.gainControl?.valueDb).toBe(12);
  expect(updateRoutingNodeGain(limited, 'gain', 12)).toBe(limited);
  const quiet = updateRoutingNodeGain(limited, 'gain', -100);
  expect(node(quiet).runtime?.config).toMatchObject({ gainDb: -60 });
  expect(updateRoutingNodeGain(quiet, 'missing', 0)).toBe(quiet);
});

it('rehydrates default mixer ports and ignores non-string input IDs from malformed state', () => {
  const model = runtimeGraphToAudioGraph(
    documentWith(
      { id: 'mixer-a', type: 'mixer', label: null, config: { inputs: ['voice', 2, 'music'] } },
      { id: 'mixer-b', type: 'mixer', label: 'Empty', config: null },
    ),
  );
  expect(node(model).appearance.title).toBe('Mixer');
  expect(node(model).inputPorts.map((port) => port.runtimePortId)).toEqual([
    'voice',
    'voice',
    'music',
    'music',
  ]);
  expect(model.nodes[1]?.inputPorts).toEqual([]);
  expect(audioGraphToRuntimeGraph(model, empty).nodes[1]?.config).toBeNull();
});

it.each(['compressor', 'limiter'] as const)(
  'renders incomplete native %s state with safe defaults while retaining its original config',
  (type) => {
    const original = documentWith({ id: type, type, label: null, config: null });
    const model = runtimeGraphToAudioGraph(original);
    expect(
      node(model).appearance.parameterControls?.every((control) => Number.isFinite(control.value)),
    ).toBe(true);
    expect(node(model).appearance.detail).toBe(
      type === 'limiter' ? '-1.0 dB ceiling' : '-18.0 dB · 4.0:1',
    );
    expect(audioGraphToRuntimeGraph(model, original).nodes[0]?.config).toBeNull();
  },
);

it('marks an installed effect with no usable layout unavailable instead of inventing ports', () => {
  const original = documentWith({
    id: 'effect-node',
    type: 'provider',
    label: null,
    config: { typeId: processor.typeId, parameters: { parameterValues: { '1': 'invalid' } } },
  });
  const model = runtimeGraphToAudioGraph(original, undefined, [{ ...processor, layouts: [] }]);
  expect(node(model).appearance).toMatchObject({ title: 'Effect', offline: true });
  expect(node(model).inputPorts).toEqual([]);
  expect(node(model).outputPorts).toEqual([]);
  expect(audioGraphToRuntimeGraph(model, original).nodes[0]?.config).toEqual(
    original.nodes[0]?.config,
  );
});

it('keeps malformed authored effect layout unavailable rather than replacing it with a catalog default', () => {
  const original = documentWith({
    id: 'effect-node',
    type: 'provider',
    label: 'Stored effect',
    config: { typeId: processor.typeId, ports: { ...ports, outputs: [] } },
  });
  const model = runtimeGraphToAudioGraph(original, undefined, [processor]);
  expect(node(model).appearance.offline).toBe(true);
  expect(node(model).outputPorts).toEqual([]);
  expect(audioGraphToRuntimeGraph(model, original).nodes[0]?.config).toEqual(
    original.nodes[0]?.config,
  );
});

it('retains unknown effect identity and authored geometry when its catalog entry disappears', () => {
  const original = documentWith({ id: 'lost-effect', type: 'provider', label: null, config: null });
  const model = runtimeGraphToAudioGraph(original);
  expect(node(model).appearance).toMatchObject({ title: 'lost-effect', offline: true });
  expect(audioGraphToRuntimeGraph(model, original).nodes).toEqual(original.nodes);
});

it('cannot create an uninstalled virtual device when the controller disallows creation', () => {
  const initial = runtimeGraphToAudioGraph(empty);
  const result = addRoutingNode(
    initial,
    { kind: 'virtual_input', spec: virtualSpec },
    position,
    undefined,
    { ...virtualPolicy, canCreate: false, devices: [] },
  );
  expect(result).toBe(initial);
  const existing = addRoutingNode(
    initial,
    { kind: 'virtual_input', spec: virtualSpec },
    position,
    undefined,
    { ...virtualPolicy, canCreate: false },
  );
  expect(existing.nodes).toHaveLength(1);
});

it('ignores a virtual endpoint without an authored render/capture reference', () => {
  const original = documentWith({
    id: 'virtual',
    type: 'virtual_endpoint',
    label: null,
    config: virtualSpec,
  });
  expect(runtimeGraphToAudioGraph(original).nodes).toEqual([]);
});

it('keeps synthetic virtual side IDs distinct from authored node IDs', () => {
  const endpointId = 'virtual';
  const escaped = '@virtual:["virtual","source"]';
  const original = documentWith(
    {
      id: endpointId,
      type: 'virtual_endpoint',
      label: null,
      config: { ...virtualSpec, reference: { render: true, capture: false } },
    },
    { id: `${endpointId}::host-source`, type: 'gain', label: null, config: {} },
    { id: escaped, type: 'gain', label: null, config: {} },
  );
  const model = runtimeGraphToAudioGraph(original);
  expect(node(model).id).toBe(`#${escaped}`);
  expect(new Set(model.nodes.map((item) => item.id)).size).toBe(3);
  expect(audioGraphToRuntimeGraph(model, original).nodes.map((item) => item.id)).toEqual(
    original.nodes.map((item) => item.id),
  );
});

it('does not publish synthetic or malformed external parameter IDs', () => {
  const base = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'provider', processor, ports },
    position,
  );
  const first = node(base);
  const control = {
    label: 'Parameter',
    minimum: 0,
    maximum: 1,
    step: 0.01,
    suffix: '',
    value: 0.5,
  };
  const before = {
    ...base,
    nodes: [
      {
        ...first,
        appearance: { ...first.appearance, parameterControls: [{ ...control, id: 'external:1' }] },
      },
    ],
  };
  const after = {
    ...before,
    nodes: [
      {
        ...node(before),
        appearance: {
          ...first.appearance,
          parameterControls: ['internal', 'external:-1', 'external:1.2', 'external:nope'].map(
            (id) => ({ ...control, id, value: 0.7 }),
          ),
        },
      },
    ],
  };
  expect(changedProcessorParameters(before, after)).toEqual([]);
});

it.each([
  ['null layout', null],
  ['empty inputs', { ...ports, inputs: [] }],
  ['empty outputs', { ...ports, outputs: [] }],
  ['extra top-level field', { ...ports, unexpected: true }],
  ['null input bus', { ...ports, inputs: [null] }],
  ['mismatched stereo geometry', { ...ports, inputs: [{ ...ports.inputs[0], channels: 1 }] }],
  ['fractional channel count', { ...ports, inputs: [{ ...ports.inputs[0], channels: 1.5 }] }],
  ['negative channel count', { ...ports, inputs: [{ ...ports.inputs[0], channels: -2 }] }],
  ['unknown bus kind', { ...ports, inputs: [{ ...ports.inputs[0], kind: 'other' }] }],
  [
    'unknown channel format',
    { ...ports, inputs: [{ ...ports.inputs[0], channelFormat: 'other' }] },
  ],
  ['nul bus name', { ...ports, inputs: [{ ...ports.inputs[0], name: 'Input\0' }] }],
  ['extra bus field', { ...ports, inputs: [{ ...ports.inputs[0], extra: 1 }] }],
  ['duplicate bus IDs', { ...ports, inputs: [...ports.inputs, ...ports.inputs] }],
  ['duplicate output IDs', { ...ports, outputs: [...ports.outputs, ...ports.outputs] }],
  ['empty layout identity', { ...ports, layoutId: '' }],
  ['control character identity', { ...ports, layoutId: 'layout\n' }],
  ['oversized layout identity', { ...ports, layoutId: 'é'.repeat(129) }],
  ['non-string layout identity', { ...ports, layoutId: 5 }],
  ['non-array outputs', { ...ports, outputs: null }],
  ['non-string bus identity', { ...ports, inputs: [{ ...ports.inputs[0], id: 4 }] }],
  ['non-string bus name', { ...ports, inputs: [{ ...ports.inputs[0], name: 4 }] }],
  ['oversized bus name', { ...ports, inputs: [{ ...ports.inputs[0], name: 'é'.repeat(513) }] }],
  ['non-number channel count', { ...ports, inputs: [{ ...ports.inputs[0], channels: '2' }] }],
  ['zero channel count', { ...ports, inputs: [{ ...ports.inputs[0], channels: 0 }] }],
  ['more than 64 channels', { ...ports, inputs: [{ ...ports.inputs[0], channels: 65 }] }],
  [
    'too many output buses',
    {
      ...ports,
      outputs: Array.from({ length: 65 }, (_, index) => ({
        ...ports.outputs[0],
        id: String(index),
      })),
    },
  ],
])('rejects %s before materializing effect ports', (_reason, value) => {
  expect(providerPortsFrom(value)).toBeNull();
});

it('rejects a valid-bus list whose combined channel count exceeds the direction bound', () => {
  const oversized = {
    ...ports,
    inputs: [
      { ...ports.inputs[0], id: 'a', channels: 40, channelFormat: 'discrete' },
      { ...ports.inputs[0], id: 'b', channels: 40, channelFormat: 'discrete' },
    ],
  };
  expect(providerPortsFrom(oversized)).toBeNull();
});

it.each(['audio_device_input', 'audio_device_output', 'application_capture'])(
  'renders incomplete %s without fabricating a selector in its runtime document',
  (type) => {
    const original = documentWith({ id: 'legacy', type, label: null, config: null });
    const model = runtimeGraphToAudioGraph(original);
    expect(node(model).appearance.title).toBe('legacy');
    expect(node(model).appearance.channelCount).toBe(2);
    expect(audioGraphToRuntimeGraph(model, original).nodes[0]?.config).toBeNull();
  },
);

it('preserves a source node represented by a view without runtime metadata', () => {
  const original = documentWith({ id: 'gain', type: 'gain', label: 'Gain', config: { gainDb: 0 } });
  const model = runtimeGraphToAudioGraph(original);
  const viewOnly = { ...model, nodes: [{ ...node(model), runtime: undefined }] };
  expect(audioGraphToRuntimeGraph(viewOnly, original).nodes).toEqual(original.nodes);
});

it('does not write gain or parameters into an invalid config object', () => {
  const model = runtimeGraphToAudioGraph(
    documentWith({ id: 'gain', type: 'gain', label: null, config: null }),
  );
  expect(updateRoutingNodeGain(model, 'gain', 4)).toBe(model);
  expect(updateRoutingNodeParameter(model, 'missing', 'external:1', 0.2)).toBe(model);
});

it('can edit a normalized external parameter even when the stored values object is missing', () => {
  const descriptor = {
    ...processor,
    parameters: [
      {
        canAutomate: true,
        defaultValue: 0.5,
        id: 1,
        isReadOnly: false,
        name: 'Amount',
        stepCount: 100,
      },
    ],
  };
  const original = documentWith({
    id: 'effect',
    type: 'provider',
    label: null,
    config: { typeId: descriptor.typeId, ports, parameters: null },
  });
  const model = runtimeGraphToAudioGraph(original, undefined, [descriptor]);
  const changed = updateRoutingNodeParameter(model, 'effect', 'external:1', 0.7);
  expect(audioGraphToRuntimeGraph(changed, original).nodes[0]?.config).toMatchObject({
    parameters: { parameterValues: { '1': expect.closeTo(0.7, 12) } },
  });
  expect(updateRoutingNodeParameter(changed, 'effect', 'external:1', 0.7)).toBe(changed);
});

it('keeps unchanged and missing endpoint edits idempotent', () => {
  const model = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_input', spec: virtualSpec },
    position,
  );
  const update = {
    displayName: virtualSpec.displayName,
    renderChannels: 64,
    captureChannels: 16,
  };
  expect(updateVirtualEndpoint(model, 'missing', update)).toBe(model);
  expect(updateVirtualEndpoint(model, node(model).id, { ...update, displayName: '   ' })).toBe(
    model,
  );
  expect(updateVirtualEndpoint(model, node(model).id, update)).toBe(model);
});

it('does not reinterpret a hardware node as a virtual endpoint on an invalid edit', () => {
  const model = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'audio_output', title: 'Speakers', uid: 'speakers', channels: 2 },
    position,
  );
  expect(
    updateVirtualEndpoint(model, node(model).id, {
      displayName: 'Output',
      renderChannels: 2,
      captureChannels: 2,
    }),
  ).toBe(model);
});

it('does not materialize a side whose native endpoint has no channels for that direction', () => {
  const model = runtimeGraphToAudioGraph(empty);
  const spec = { ...virtualSpec, capture: { channels: 0, channelLayout: 'discrete' as const } };
  expect(addRoutingNode(model, { kind: 'virtual_input', spec }, position)).toBe(model);
});

it('allocates a distinct recorder identity and keeps other nodes when resizing its inputs', () => {
  const first = addRoutingNode(runtimeGraphToAudioGraph(empty), { kind: 'recorder' }, position);
  const second = addRoutingNode(first, { kind: 'recorder' }, position);
  expect(second.nodes.map((item) => item.id)).toEqual(['recorder-1', 'recorder-2']);
  const changed = updateRecorderChannels(second, 'recorder-2', 1);
  expect(changed.nodes[0]).toBe(second.nodes[0]);
  expect(changed.nodes[1]?.inputPorts).toHaveLength(1);
  expect(audioGraphToRuntimeGraph(changed, empty).nodes[1]?.config).toEqual({ channels: 1 });
});

it('accepts unnamed processor buses and falls back to their bus kind', () => {
  const unnamed = {
    ...ports,
    inputs: ports.inputs.map((bus) => ({ ...bus, name: '' })),
    outputs: ports.outputs.map((bus) => ({ ...bus, name: '' })),
  };
  const model = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'provider', processor, ports: unnamed },
    position,
  );
  expect(node(model).inputPorts[0]?.label).toBe('Main 1');
  expect(node(model).outputPorts[0]?.label).toBe('Main 1');
});

it('does not invent parameter changes when a restored effect has no authored controls', () => {
  const model = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'provider', processor, ports },
    position,
  );
  expect(changedProcessorParameters(model, model)).toEqual([]);
  expect(updateRoutingNodeParameter(model, node(model).id, 'external:0', 0.5)).toBe(model);
});

it('uses the default recorder title for a saved unnamed recorder', () => {
  const model = runtimeGraphToAudioGraph(
    documentWith({ id: 'rec', type: 'recorder', label: null, config: null }),
  );
  expect(node(model).appearance.title).toBe('Recorder');
  expect(node(model).inputPorts).toHaveLength(2);
});

it('rejects an endpoint edit when the restored device identity is missing', () => {
  const model = runtimeGraphToAudioGraph(
    documentWith({
      id: 'virtual',
      type: 'virtual_endpoint',
      label: 'Device',
      config: { ...virtualSpec, id: '', reference: { render: true, capture: true } },
    }),
  );
  expect(
    updateVirtualEndpoint(model, node(model).id, {
      displayName: 'Updated',
      renderChannels: 2,
      captureChannels: 2,
    }),
  ).toBe(model);
});

it.each(['render', 'capture'] as const)(
  'keeps a native %s-only device single-directional when editing channels',
  (direction) => {
    const spec =
      direction === 'render'
        ? { ...virtualSpec, capture: undefined }
        : { ...virtualSpec, render: undefined };
    const model = addRoutingNode(
      runtimeGraphToAudioGraph(empty),
      { kind: direction === 'render' ? 'virtual_output' : 'virtual_input', spec },
      position,
    );
    const update = {
      displayName: 'Updated',
      renderChannels: direction === 'render' ? 1 : 0,
      captureChannels: direction === 'capture' ? 1 : 0,
    };
    const changed = updateVirtualEndpoint(model, node(model).id, update);
    expect(node(changed).appearance.title).toBe('Updated');
    const unexpected = {
      ...update,
      [direction === 'render' ? 'captureChannels' : 'renderChannels']: 1,
    };
    expect(updateVirtualEndpoint(changed, node(changed).id, unexpected)).toBe(changed);
  },
);

it('keeps generic parameter controls editable without inventing a built-in description', () => {
  const created = addRoutingNode(runtimeGraphToAudioGraph(empty), { kind: 'compressor' }, position);
  const first = node(created);
  if (!first.runtime) throw new Error('fixture runtime');
  const generic = {
    ...created,
    nodes: [{ ...first, runtime: { ...first.runtime, type: 'custom_processor' } }],
  };
  const changed = updateRoutingNodeParameter(generic, first.id, 'thresholdDb', -12);
  expect(node(changed).runtime?.config).toMatchObject({ thresholdDb: -12 });
  expect(node(changed).appearance.detail).toBeUndefined();
});

it('does not infer an application icon after the captured application needs reselection', () => {
  const original = documentWith({
    id: 'app',
    type: 'application_capture',
    label: 'App',
    config: {
      requiresReselection: true,
      selector: { match: 'bundle_id', bundleId: 'example.app' },
      channels: 2,
    },
  });
  const model = runtimeGraphToAudioGraph(original);
  const marked = {
    ...model,
    nodes: model.nodes.map((item) => ({
      ...item,
      appearance: { ...item.appearance, iconUrl: 'stale' },
    })),
  };
  expect(node(presentRoutingApplicationIcons(marked, [])).appearance.iconUrl).toBeUndefined();
});

it('serializes a new virtual reference without a source document and repairs missing metadata on edit', () => {
  const created = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_input', spec: virtualSpec },
    position,
  );
  expect(audioGraphToRuntimeGraph(created, empty).nodes[0]?.label).toBe(virtualSpec.displayName);
  const first = node(created);
  if (!first.runtime) throw new Error('fixture runtime');
  const malformed = {
    ...created,
    nodes: [{ ...first, runtime: { ...first.runtime, config: null } }],
  };
  expect(audioGraphToRuntimeGraph(malformed, empty).nodes[0]?.config).toMatchObject({
    reference: { render: true, capture: false },
  });
  const unnamedConfig = { ...virtualSpec, displayName: undefined, render: 'legacy' };
  const legacy = {
    ...created,
    nodes: [{ ...first, runtime: { ...first.runtime, config: unnamedConfig } }],
  };
  const changed = updateVirtualEndpoint(legacy, first.id, {
    displayName: first.appearance.title,
    renderChannels: 1,
    captureChannels: 1,
  });
  expect(node(changed).runtime?.config).toMatchObject({
    render: { channels: 1, channelLayout: 'mono' },
  });
});

it('detects parameter changes when a processor descriptor newly exposes or removes controls', () => {
  const created = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'provider', processor, ports },
    position,
  );
  const first = node(created);
  const after = {
    ...created,
    nodes: [
      {
        ...first,
        appearance: {
          ...first.appearance,
          parameterControls: [
            {
              id: 'external:9',
              label: 'Wet',
              minimum: 0,
              maximum: 1,
              step: 0.01,
              suffix: '',
              value: 0.5,
            },
          ],
        },
      },
    ],
  };
  const before = {
    ...created,
    nodes: [{ ...first, appearance: { ...first.appearance, parameterControls: undefined } }],
  };
  expect(changedProcessorParameters(before, after)).toEqual([
    { nodeId: first.id, typeId: 'effect', parameterId: 9, value: 0.5 },
  ]);
  expect(changedProcessorParameters(after, before)).toEqual([]);
});

it('removes dangling UI edges when recorder geometry changes', () => {
  const model = addRoutingNode(runtimeGraphToAudioGraph(empty), { kind: 'recorder' }, position);
  const first = node(model);
  const stale = {
    ...model,
    edges: [
      {
        id: 'missing-target',
        sourceNodeId: first.id,
        sourcePortId: 'audio:0',
        targetNodeId: 'removed',
        targetPortId: 'audio:0',
      },
    ],
  };
  expect(updateRecorderChannels(stale, first.id, 1).edges).toEqual([]);
});

it('preserves legacy cable identities with channel zero when port metadata is unavailable', () => {
  const original = documentWith({ id: 'gain', type: 'gain', label: null, config: { channels: 2 } });
  const model = runtimeGraphToAudioGraph(original);
  const first = node(model);
  const legacy = {
    ...model,
    nodes: [{ ...first, outputPorts: [{ id: 'legacy-out', label: 'Output' }] }],
    edges: [
      {
        id: 'legacy',
        sourceNodeId: first.id,
        sourcePortId: 'legacy-out',
        targetNodeId: 'absent',
        targetPortId: 'legacy-in',
      },
    ],
  };
  expect(audioGraphToRuntimeGraph(legacy, original).edges[0]).toMatchObject({
    source: { nodeId: 'gain', portId: 'legacy-out' },
    destination: { nodeId: 'absent', portId: 'legacy-in' },
    channelMapping: { mode: 'explicit', routes: [{ sourceChannel: 0, destinationChannel: 0 }] },
  });
});

it('retains a legacy endpoint display label when its stored display name is absent', () => {
  const config = {
    ...virtualSpec,
    displayName: undefined,
    reference: { render: true, capture: true },
  };
  const model = runtimeGraphToAudioGraph(
    documentWith({
      id: 'virtual',
      type: 'virtual_endpoint',
      label: virtualSpec.displayName,
      config,
    }),
  );
  expect(
    updateVirtualEndpoint(model, node(model).id, {
      displayName: node(model).appearance.title,
      renderChannels: 64,
      captureChannels: 16,
    }),
  ).toBe(model);
});

it('does not offer non-existent directions in a single-direction endpoint policy', () => {
  const renderOnly = { ...virtualSpec, capture: undefined };
  const model = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_output', spec: renderOnly },
    position,
  );
  const shown = presentVirtualEndpointSettings(model, virtualPolicy);
  expect(node(shown).appearance.endpointControl?.allowedCaptureChannels).toEqual([]);
  expect(node(shown).appearance.endpointControl?.allowedRenderChannels).toEqual(
    virtualPolicy.renderChannels,
  );
});

it('removes a dangling target even when its source port still exists after geometry edits', () => {
  const first = addRoutingNode(runtimeGraphToAudioGraph(empty), { kind: 'recorder' }, position);
  const created = addRoutingNode(first, { kind: 'virtual_input', spec: virtualSpec }, position);
  const source = created.nodes.find((item) => item.outputPorts.length);
  if (!source?.outputPorts[0]) throw new Error('source fixture');
  const stale = {
    ...created,
    edges: [
      {
        id: 'missing-target',
        sourceNodeId: source.id,
        sourcePortId: source.outputPorts[0].id,
        targetNodeId: 'removed',
        targetPortId: 'audio:0',
      },
    ],
  };
  expect(updateRecorderChannels(stale, node(stale).id, 1).edges).toEqual([]);
});
