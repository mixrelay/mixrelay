import type { Meta, StoryObj } from '@storybook/react-vite';

import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import type { RuntimeGraphDocument } from '../../core/control-port/types';
import { RoutingWorkspace } from './RoutingWorkspace';

const graph: RuntimeGraphDocument = {
  edges: [],
  nodes: [
    {
      config: {
        reference: { render: true, capture: true },
        capture: { channelLayout: 'stereo', channels: 2 },
        defaultSampleRate: 48_000,
        displayName: 'MixRelay Virtual I/O',
        id: 'default-virtual-io',
        render: { channelLayout: 'stereo', channels: 2 },
        supportedSampleRates: [48_000],
      },
      id: 'default-virtual-io',
      label: 'MixRelay Virtual I/O',
      type: 'virtual_endpoint',
    },
  ],
  revision: 1,
  schemaVersion: 2,
};

const controlPort = new StorybookMockControlPort({
  graph,
  graphStatus: { activeRevision: 1, protocolVersion: 1, submittedRevision: 1 },
  meters: {
    nodes: [{ nodeId: 'default-virtual-io', peaks: [0.42, 0.19], portId: 'from_os' }],
    protocolVersion: 1,
    revision: 1,
  },
  status: {
    bufferFrames: 128,
    driverState: 'ready',
    overruns: 0,
    protocolVersion: 1,
    sampleRate: 48_000,
    underruns: 0,
  },
});

const applicationGraph: RuntimeGraphDocument = {
  ...graph,
  edges: [
    {
      channelMapping: { mode: 'auto' },
      destination: { nodeId: 'gain-1', portId: 'audio' },
      id: 'parsec-to-gain',
      source: { nodeId: 'application-1', portId: 'audio' },
    },
    {
      channelMapping: { mode: 'auto' },
      destination: { nodeId: 'gain-1', portId: 'audio' },
      id: 'safari-to-gain',
      source: { nodeId: 'application-2', portId: 'audio' },
    },
    {
      channelMapping: { mode: 'auto' },
      destination: { nodeId: 'default-virtual-io', portId: 'to_os' },
      id: 'gain-to-virtual',
      source: { nodeId: 'gain-1', portId: 'audio' },
    },
  ],
  nodes: [
    ...graph.nodes,
    {
      config: {
        gainDb: -3,
        selector: {
          bundle_id: 'tv.parsec.www',
          include_descendants: true,
          match: 'bundle_id',
        },
        sourcePlayback: 'preserve',
        stream: { channelLayout: 'stereo', channels: 2 },
      },
      id: 'application-1',
      label: 'Parsec',
      type: 'application_capture',
    },
    {
      config: {
        gainDb: -6,
        selector: {
          bundle_id: 'com.apple.Safari',
          include_descendants: true,
          match: 'bundle_id',
        },
        sourcePlayback: 'preserve',
        stream: { channelLayout: 'stereo', channels: 2 },
      },
      id: 'application-2',
      label: 'Safari',
      type: 'application_capture',
    },
    {
      config: { channels: 2, gainDb: -6 },
      id: 'gain-1',
      label: 'Stream trim',
      type: 'gain',
    },
  ],
  revision: 2,
};

const applicationControlPort = new StorybookMockControlPort({
  graph: applicationGraph,
  graphStatus: { activeRevision: 2, protocolVersion: 1, submittedRevision: 2 },
  meters: {
    nodes: [
      { nodeId: 'application-1', peaks: [0.61, 0.48], portId: 'audio' },
      { nodeId: 'application-2', peaks: [0.37, 0.29], portId: 'audio' },
      { nodeId: 'gain-1', peaks: [0.31, 0.24], portId: 'audio' },
      { nodeId: 'default-virtual-io', peaks: [0.31, 0.24], portId: 'to_os' },
    ],
    protocolVersion: 1,
    revision: 2,
  },
  status: {
    bufferFrames: 128,
    driverState: 'ready',
    overruns: 0,
    protocolVersion: 1,
    sampleRate: 48_000,
    underruns: 0,
  },
});

const meta = {
  title: 'Features/Routing/Workspace',
  component: RoutingWorkspace,
  args: { controlPort },
  parameters: { layout: 'fullscreen' },
} satisfies Meta<typeof RoutingWorkspace>;

export default meta;
type Story = StoryObj<typeof meta>;

export const VirtualIoPassThrough: Story = {};
export const ApplicationToVirtualInput: Story = {
  args: { controlPort: applicationControlPort },
};
export const MobileNarrow: Story = {
  parameters: { viewport: { defaultViewport: 'mobile' } },
};
export const LongTranslation: Story = { globals: { locale: 'pseudo' } };
