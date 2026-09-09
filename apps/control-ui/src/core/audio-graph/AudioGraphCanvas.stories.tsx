import type { Meta, StoryObj } from '@storybook/react-vite';
import { useState } from 'react';

import { AudioGraphCanvas } from './AudioGraphCanvas';
import type { AudioGraphModel } from './model';

const broadcastMix: AudioGraphModel = {
  revision: 1,
  nodes: [
    {
      id: 'chrome',
      position: { x: 40, y: 80 },
      inputPorts: [],
      outputPorts: [{ id: 'audio', label: 'Chrome audio output' }],
      appearance: {
        category: 'Source',
        detail: 'Chrome · Tab audio',
        icon: 'APP',
        status: 'Live',
        title: 'Chrome',
      },
    },
    {
      id: 'microphone',
      position: { x: 40, y: 260 },
      inputPorts: [],
      outputPorts: [{ id: 'audio', label: 'Microphone output' }],
      appearance: {
        category: 'Source',
        detail: 'Scarlett 2i2 · Input 1',
        icon: 'MIC',
        title: 'Microphone',
      },
    },
    {
      id: 'mixer',
      position: { x: 390, y: 150 },
      inputPorts: [
        { id: 'chrome', label: 'Chrome mixer input' },
        { id: 'microphone', label: 'Microphone mixer input' },
      ],
      outputPorts: [{ id: 'audio', label: 'Mixer output' }],
      appearance: {
        category: 'Processor',
        detail: 'Chrome + microphone',
        icon: 'MIX',
        title: 'Broadcast mix',
      },
    },
    {
      id: 'stream',
      position: { x: 750, y: 150 },
      inputPorts: [{ id: 'to_os', label: 'Virtual device input' }],
      outputPorts: [],
      appearance: {
        category: 'Output',
        detail: 'ToneNari Stream · 48 kHz',
        icon: 'IO',
        status: 'Ready',
        title: 'OBS Stream',
      },
    },
  ],
  edges: [
    {
      id: 'chrome-to-mixer',
      sourceNodeId: 'chrome',
      sourcePortId: 'audio',
      targetNodeId: 'mixer',
      targetPortId: 'chrome',
    },
    {
      id: 'microphone-to-mixer',
      sourceNodeId: 'microphone',
      sourcePortId: 'audio',
      targetNodeId: 'mixer',
      targetPortId: 'microphone',
    },
    {
      id: 'mixer-to-stream',
      sourceNodeId: 'mixer',
      sourcePortId: 'audio',
      targetNodeId: 'stream',
      targetPortId: 'to_os',
    },
  ],
};

const meta = {
  title: 'Core/Audio Graph/Canvas',
  component: AudioGraphCanvas,
  args: {
    ariaLabel: 'ToneNari audio routing playground',
    model: broadcastMix,
    onModelChange: () => undefined,
  },
  parameters: { layout: 'fullscreen' },
  render: () => <InteractiveGraph />,
} satisfies Meta<typeof AudioGraphCanvas>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ConnectDisconnectReconnect: Story = {};

function InteractiveGraph() {
  const [model, setModel] = useState(broadcastMix);
  return (
    <div style={{ width: '100vw', height: '100vh', padding: 24 }}>
      <AudioGraphCanvas
        ariaLabel="ToneNari audio routing playground"
        model={model}
        onModelChange={setModel}
      />
    </div>
  );
}
