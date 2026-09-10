import { expect, it } from 'vitest';

import type { AudioGraphModel } from '../../core/audio-graph/model';
import type {
  ExternalProcessorDescriptor,
  RuntimeGraphDocument,
  RuntimeGraphNode,
} from '../../core/control-port/types';
import {
  addRoutingNode,
  audioGraphToRuntimeGraph,
  runtimeGraphToAudioGraph,
  updateRoutingNodeParameter,
} from './model';
import { duplexReference, virtualSpec } from './virtualEndpointFixtures';

const DEFAULT_ENDPOINT_NODE_ID = 'test-boundary';
const spec = {
  ...virtualSpec,
  displayName: 'Endpoint name',
  render: { channelLayout: 'stereo' as const, channels: 2 },
  capture: { channelLayout: 'stereo' as const, channels: 2 },
};

function endpoint(): RuntimeGraphNode {
  return {
    config: {
      ...spec,
      reference: duplexReference,
      capture: { channelLayout: 'stereo', channels: 2 },
      render: { channelLayout: 'stereo', channels: 2 },
    },
    id: DEFAULT_ENDPOINT_NODE_ID,
    label: null,
    type: 'virtual_endpoint',
  };
}

function directDocument(): RuntimeGraphDocument {
  return {
    edges: [
      {
        channelMapping: { mode: 'auto' },
        destination: { nodeId: DEFAULT_ENDPOINT_NODE_ID, portId: 'to_os' },
        id: 'direct',
        source: { nodeId: DEFAULT_ENDPOINT_NODE_ID, portId: 'from_os' },
      },
    ],
    nodes: [endpoint()],
    revision: 4,
    schemaVersion: 2,
  };
}

function gainFixture(): RuntimeGraphDocument {
  return {
    ...directDocument(),
    nodes: [
      endpoint(),
      {
        id: 'output-gain',
        label: 'User gain label',
        type: 'gain',
        config: { channels: 2, gainDb: -6.0206003 },
      },
    ],
    edges: [
      {
        id: 'gain-in',
        source: { nodeId: DEFAULT_ENDPOINT_NODE_ID, portId: 'from_os' },
        destination: { nodeId: 'output-gain', portId: 'audio' },
        channelMapping: { mode: 'auto' },
      },
      {
        id: 'gain-out',
        source: { nodeId: 'output-gain', portId: 'audio' },
        destination: { nodeId: DEFAULT_ENDPOINT_NODE_ID, portId: 'to_os' },
        channelMapping: { mode: 'auto' },
      },
    ],
  };
}

it.each(['virtual_input', 'virtual_output'] as const)(
  'recreates %s from an empty graph with the required endpoint identity',
  (kind) => {
    const empty: RuntimeGraphDocument = { schemaVersion: 2, revision: 1, nodes: [], edges: [] };
    const model = addRoutingNode(runtimeGraphToAudioGraph(empty), { kind, spec }, { x: 40, y: 80 });
    const serialized = audioGraphToRuntimeGraph(model, empty);
    expect(serialized.nodes).toEqual([
      {
        id: 'virtual-1',
        label: spec.displayName,
        type: 'virtual_endpoint',
        config: {
          ...spec,
          gainDb: 0,
          reference: {
            render: kind === 'virtual_input',
            capture: kind === 'virtual_output',
          },
        },
      },
    ]);
    const both = addRoutingNode(
      runtimeGraphToAudioGraph(serialized),
      {
        kind: kind === 'virtual_input' ? 'virtual_output' : 'virtual_input',
        spec,
      },
      { x: 400, y: 80 },
    );
    const reunited = audioGraphToRuntimeGraph(both, serialized);
    expect(reunited.nodes).toHaveLength(1);
    expect(reunited.nodes[0]?.config).toMatchObject({
      id: spec.id,
      render: { channels: 2 },
      capture: { channels: 2 },
    });
  },
);

it('maps the duplex OS boundary and gain node without inventing runtime configuration', () => {
  const gainDocument = gainFixture();
  const model = runtimeGraphToAudioGraph(gainDocument);

  expect(model.nodes[0]?.appearance.title).toBe(spec.displayName);
  expect(model.nodes[0]?.outputPorts[0]?.boundary).toBe('source');
  expect(model.nodes[1]?.inputPorts[0]?.boundary).toBe('sink');
  expect(model.nodes[2]?.appearance.detail).toBe('-6.0 dB');
  expect(model.nodes[0]?.outputPorts).toHaveLength(2);
  expect(model.nodes[1]?.inputPorts).toHaveLength(2);
  expect(model.edges).toHaveLength(4);

  const restored = audioGraphToRuntimeGraph(model, gainDocument);
  expect(restored.nodes).toEqual(gainDocument.nodes);
  expect(restored.edges).toEqual(gainDocument.edges);
});

it('preserves user labels even for IDs used by former demonstration fixtures', () => {
  const original = gainFixture();
  const model = runtimeGraphToAudioGraph(original);
  expect(model.nodes.find((node) => node.id === 'output-gain')?.appearance.title).toBe(
    'User gain label',
  );
  expect(audioGraphToRuntimeGraph(model, original).nodes).toEqual(original.nodes);
});

it('renders unknown runtime nodes safely and drops UI-only nodes on serialization', () => {
  const document: RuntimeGraphDocument = {
    ...directDocument(),
    nodes: [
      { config: { gainDb: 'invalid' }, id: 'bad-gain', label: null, type: 'gain' },
      { config: null, id: 'empty-gain', label: 'Empty', type: 'gain' },
      { config: {}, id: 'future', label: null, type: 'future_node' },
    ],
  };
  const model = runtimeGraphToAudioGraph(document);
  expect(model.nodes[0]?.appearance.detail).toBe('0.0 dB');
  expect(model.nodes[1]?.appearance.detail).toBe('0.0 dB');
  expect(model.nodes[2]?.appearance.category).toBe('Unsupported');

  const withUiOnlyNode: AudioGraphModel = {
    ...model,
    nodes: [
      ...model.nodes,
      {
        appearance: { category: 'Test', detail: '', icon: '?', title: 'UI only' },
        id: 'ui-only',
        inputPorts: [],
        outputPorts: [],
        position: { x: 0, y: 0 },
      },
    ],
  };
  expect(audioGraphToRuntimeGraph(withUiOnlyNode, document).nodes).toHaveLength(3);
});

it('materializes an external processor and persists normalized parameter changes', () => {
  const processor: ExternalProcessorDescriptor = {
    displayName: 'Fixture Compressor',
    format: 'vst3',
    hasEditor: true,
    inputChannels: 2,
    outputChannels: 2,
    parameters: [
      {
        canAutomate: true,
        defaultValue: 0.5,
        id: 42,
        isReadOnly: false,
        name: 'Threshold',
        stepCount: 100,
      },
    ],
    typeId: 'vst3:00112233445566778899AABBCCDDEEFF',
    vendor: 'ToneNari Test',
    version: '1.0',
  };
  const empty: AudioGraphModel = { edges: [], nodes: [], revision: 1 };
  const added = addRoutingNode(empty, { kind: 'provider', processor }, { x: 40, y: 80 });
  const node = added.nodes[0];
  expect(node?.appearance.title).toBe('Fixture Compressor');
  expect(node?.appearance.meterChannels).toBeUndefined();
  expect(node?.appearance.meterSource).toBeUndefined();
  expect(node?.inputPorts).toHaveLength(2);
  expect(node?.outputPorts).toHaveLength(2);
  expect(node?.appearance.parameterControls?.[0]).toMatchObject({
    id: 'external:42',
    value: 0.5,
  });

  const changed = updateRoutingNodeParameter(added, node?.id ?? '', 'external:42', 0.73);
  const serialized = audioGraphToRuntimeGraph(changed, {
    edges: [],
    nodes: [],
    revision: 1,
    schemaVersion: 2,
  });
  expect(serialized.nodes[0]?.config).toMatchObject({
    parameters: { parameterValues: { '42': 0.73 }, version: 1 },
    typeId: processor.typeId,
  });

  const restored = runtimeGraphToAudioGraph(serialized, undefined, [processor]);
  expect(restored.nodes[0]?.appearance.meterSource).toBeUndefined();
  expect(restored.nodes[0]?.appearance.parameterControls?.[0]?.value).toBe(0.73);
});
