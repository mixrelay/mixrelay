import { expect, it } from 'vitest';
import type {
  ExternalProcessorDescriptor,
  RuntimeGraphDocument,
} from '../../core/control-port/types';
import { serializeGraphEdges } from './graphEdgeProjection';
import {
  addRoutingNode,
  audioGraphToRuntimeGraph,
  runtimeGraphToAudioGraph,
  updateVirtualEndpoint,
} from './model';
import { duplexReference, virtualSpec } from './virtualEndpointFixtures';
import { virtualEndpointNodeRequest } from './virtualEndpointMenu';

const empty: RuntimeGraphDocument = { schemaVersion: 2, revision: 1, nodes: [], edges: [] };
const position = { x: 0, y: 0 };

it.each(['render', 'capture'] as const)(
  'renames a %s-only device without creating its absent stream',
  (side) => {
    const spec = {
      ...virtualSpec,
      render: side === 'render' ? virtualSpec.render : null,
      capture: side === 'capture' ? virtualSpec.capture : null,
    };
    const request = virtualEndpointNodeRequest('input', spec);
    if (!request) throw new Error('Expected legal source');
    const model = addRoutingNode(runtimeGraphToAudioGraph(empty), request, position);
    const source = model.nodes[0];
    if (!source) throw new Error('Expected source view');
    expect(source.outputPorts).toHaveLength(
      Math.max(spec.render?.channels ?? 0, spec.capture?.channels ?? 0),
    );
    expect(source.outputPorts.every((port) => port.runtimePortId === 'from_os')).toBe(true);
    const renamed = updateVirtualEndpoint(model, source.id, {
      displayName: 'Renamed',
      renderChannels: spec.render?.channels ?? 0,
      captureChannels: spec.capture?.channels ?? 0,
    });
    expect(audioGraphToRuntimeGraph(renamed, empty).nodes[0]?.config).toMatchObject({
      displayName: 'Renamed',
    });
    expect(audioGraphToRuntimeGraph(renamed, empty).nodes[0]?.config).toEqual(
      expect.objectContaining({ render: spec.render, capture: spec.capture }),
    );
  },
);

it.each(['source', 'sink'] as const)(
  'removes only the deleted %s from unresolved routes and filters removed channels individually',
  (removedSide) => {
    const graph: RuntimeGraphDocument = {
      ...empty,
      nodes: [
        {
          id: 'device',
          label: null,
          type: 'virtual_endpoint',
          config: { ...virtualSpec, reference: duplexReference },
        },
        { id: 'fx', label: null, type: 'provider', config: { typeId: 'missing' } },
      ],
      edges: [
        {
          id: 'into-device',
          source: { nodeId: 'fx', portId: 'audio' },
          destination: { nodeId: 'device', portId: 'to_os' },
          channelMapping: {
            mode: 'explicit',
            routes: [
              { sourceChannel: 0, destinationChannel: 0 },
              { sourceChannel: 1, destinationChannel: 13 },
            ],
          },
        },
        {
          id: 'out-device',
          source: { nodeId: 'device', portId: 'from_os' },
          destination: { nodeId: 'fx', portId: 'audio' },
          channelMapping: { mode: 'auto' },
        },
      ],
    };
    const model = runtimeGraphToAudioGraph(graph);
    const removedSink = {
      ...model,
      nodes: model.nodes.filter((node) => node.runtime?.binding?.side !== removedSide),
    };
    expect(audioGraphToRuntimeGraph(removedSink, graph).edges).toEqual([
      graph.edges[removedSide === 'sink' ? 1 : 0],
    ]);
    const source = model.nodes.find((node) => node.runtime?.binding?.side === 'source');
    if (!source) throw new Error('Source fixture missing');
    const resized = updateVirtualEndpoint(model, source.id, {
      displayName: virtualSpec.displayName,
      renderChannels: 64,
      captureChannels: 2,
    });
    expect(audioGraphToRuntimeGraph(resized, graph).edges).toEqual([
      {
        ...graph.edges[0],
        channelMapping: { mode: 'explicit', routes: [{ sourceChannel: 0, destinationChannel: 0 }] },
      },
      graph.edges[1],
    ]);
  },
);

it('retains edited provider state and nullable labels when its descriptor becomes unavailable', () => {
  const original: RuntimeGraphDocument = {
    ...empty,
    nodes: [
      {
        id: 'fx',
        label: null,
        type: 'provider',
        config: { typeId: 'fixture', parameters: { parameterValues: { '42': 0.2 } } },
      },
    ],
  };
  const draft: RuntimeGraphDocument = {
    ...original,
    nodes: [
      {
        ...original.nodes[0],
        id: 'fx',
        label: null,
        type: 'provider',
        config: { typeId: 'fixture', parameters: { parameterValues: { '42': 0.8 } } },
      },
    ],
  };
  const placeholder = runtimeGraphToAudioGraph(draft);
  expect(placeholder.nodes[0]?.outputPorts).toEqual([]);
  expect(audioGraphToRuntimeGraph(placeholder, original).nodes).toEqual(draft.nodes);
});

it('does not collide edited cable IDs with preserved authoritative route IDs', () => {
  const original = {
    id: 'existing',
    source: { nodeId: 'a', portId: 'audio' },
    destination: { nodeId: 'b', portId: 'audio' },
    channelMapping: { mode: 'auto' as const },
  };
  const cable = {
    id: 'existing',
    sourceNodeId: 'a',
    sourcePortId: 'audio:0',
    targetNodeId: 'b',
    targetPortId: 'audio:0',
  };
  const result = serializeGraphEdges(
    { nodes: [], revision: 1, edges: [cable], edgeProjections: [{ edge: original, views: [] }] },
    new Set(['a', 'b']),
    () => original,
  );
  expect(result[0]).toEqual(original);
  expect(result).toHaveLength(2);
  expect(new Set(result.map((edge) => edge.id)).size).toBe(2);
});

it('keeps N identities distinct while sharing each identity between its input and output references', () => {
  let model = runtimeGraphToAudioGraph(empty);
  const specs = [virtualSpec, { ...virtualSpec, id: '3588a3be-bd13-48ec-ae63-edc7e00ef873' }];
  for (const spec of specs) {
    model = addRoutingNode(model, { kind: 'virtual_input', spec }, position);
    model = addRoutingNode(model, { kind: 'virtual_output', spec }, position);
  }
  const saved = audioGraphToRuntimeGraph(model, empty);
  expect(model.nodes).toHaveLength(4);
  expect(saved.nodes.map((node) => node.config)).toEqual(
    specs.map((spec) => ({ ...spec, gainDb: 0, reference: duplexReference })),
  );
  expect(new Set(saved.nodes.map((node) => node.id)).size).toBe(2);
  const otherGraph = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_output', spec: virtualSpec },
    position,
  );
  expect(audioGraphToRuntimeGraph(otherGraph, empty).nodes[0]?.config).toMatchObject({
    id: virtualSpec.id,
  });
});

it('removes references without shrinking the native device and removes the graph node only after its final view', () => {
  let model = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_input', spec: virtualSpec },
    position,
  );
  model = addRoutingNode(model, { kind: 'virtual_output', spec: virtualSpec }, position);
  const remaining = {
    ...model,
    nodes: model.nodes.filter((node) => node.runtime?.binding?.side === 'sink'),
  };
  expect(audioGraphToRuntimeGraph(remaining, empty).nodes[0]?.config).toMatchObject({
    render: virtualSpec.render,
    capture: virtualSpec.capture,
    reference: { render: false, capture: true },
  });
  expect(audioGraphToRuntimeGraph({ ...remaining, nodes: [] }, empty).nodes).toEqual([]);
});

it.each([
  [64, 16],
  [2, 32],
  [8, 8],
])('presents one combined source at max(%i, %i) channels', (renderChannels, captureChannels) => {
  const spec = {
    ...virtualSpec,
    render: { channels: renderChannels, channelLayout: 'discrete' as const },
    capture: { channels: captureChannels, channelLayout: 'discrete' as const },
  };
  const model = addRoutingNode(
    runtimeGraphToAudioGraph(empty),
    { kind: 'virtual_input', spec },
    position,
  );
  const source = model.nodes[0];
  if (!source) throw new Error('Source fixture absent');
  expect(source.outputPorts).toHaveLength(Math.max(renderChannels, captureChannels));
  expect(source.outputPorts.every((port) => port.runtimePortId === 'from_os')).toBe(true);
  const saved = audioGraphToRuntimeGraph(model, empty);
  expect(saved.nodes[0]?.config).toMatchObject({
    reference: { render: true, capture: false },
  });
  expect(runtimeGraphToAudioGraph(saved).nodes[0]?.appearance.meterSource?.portId).toBe('from_os');
});

it('never decodes ordinary node IDs as virtual suffixes, including colliding view names', () => {
  const graph: RuntimeGraphDocument = {
    ...empty,
    nodes: [
      {
        id: 'fx',
        label: null,
        type: 'virtual_endpoint',
        config: { ...virtualSpec, reference: duplexReference },
      },
      {
        id: 'fx::host-source',
        label: 'User processor',
        type: 'gain',
        config: { gainDb: 0, channels: 2 },
      },
      {
        id: '$fx',
        label: null,
        type: 'virtual_endpoint',
        config: { ...virtualSpec, id: 'second-device', reference: duplexReference },
      },
    ],
    edges: [
      {
        id: 'route',
        source: { nodeId: 'fx::host-source', portId: 'audio' },
        destination: { nodeId: 'fx', portId: 'to_os' },
        channelMapping: { mode: 'explicit', routes: [{ sourceChannel: 1, destinationChannel: 3 }] },
      },
    ],
  };
  const model = runtimeGraphToAudioGraph(graph);
  expect(new Set(model.nodes.map((node) => node.id)).size).toBe(model.nodes.length);
  const saved = audioGraphToRuntimeGraph(model, graph);
  expect(saved.nodes.map((node) => node.id)).toEqual(graph.nodes.map((node) => node.id));
  expect(saved.edges[0]).toMatchObject({
    source: graph.edges[0]?.source,
    destination: graph.edges[0]?.destination,
    channelMapping: graph.edges[0]?.channelMapping,
  });
});

it('retains unresolved provider edges through unrelated edits and restores exact channel routes when the catalog arrives', () => {
  const graph: RuntimeGraphDocument = {
    ...empty,
    nodes: [
      {
        id: 'device',
        label: null,
        type: 'virtual_endpoint',
        config: { ...virtualSpec, reference: duplexReference },
      },
      { id: 'fx', label: 'Effect', type: 'provider', config: { typeId: 'test:effect' } },
    ],
    edges: [
      {
        id: 'kept',
        source: { nodeId: 'fx', portId: 'audio' },
        destination: { nodeId: 'device', portId: 'to_os' },
        channelMapping: {
          mode: 'explicit',
          routes: [{ sourceChannel: 1, destinationChannel: 13 }],
        },
      },
    ],
  };
  const pending = runtimeGraphToAudioGraph(graph);
  expect(pending.edges).toEqual([]);
  expect(pending.nodes.find((node) => node.id === 'fx')?.outputPorts).toEqual([]);
  const edited = addRoutingNode(pending, { kind: 'compressor' }, position);
  const saved = audioGraphToRuntimeGraph(edited, graph);
  expect(saved.edges).toEqual(graph.edges);
  const descriptor: ExternalProcessorDescriptor = {
    typeId: 'test:effect',
    displayName: 'Effect',
    format: 'vst3',
    inputChannels: 2,
    outputChannels: 2,
    parameters: [],
    hasEditor: false,
    vendor: 'Test',
    version: '1',
  };
  const restored = runtimeGraphToAudioGraph(saved, undefined, [descriptor]);
  expect(restored.edges).toHaveLength(1);
  expect(audioGraphToRuntimeGraph(restored, saved).edges[0]).toMatchObject({
    source: graph.edges[0]?.source,
    destination: graph.edges[0]?.destination,
    channelMapping: graph.edges[0]?.channelMapping,
  });
  expect(
    audioGraphToRuntimeGraph(
      { ...pending, nodes: pending.nodes.filter((node) => node.id !== 'fx') },
      graph,
    ).edges,
  ).toEqual([]);
});
