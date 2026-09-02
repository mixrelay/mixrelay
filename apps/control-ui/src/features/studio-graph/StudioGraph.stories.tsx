import type { Meta, StoryObj } from '@storybook/react-vite';

import { NodeInventory, StudioGraph } from './StudioGraph';

const meta = {
  title: 'Feature/Studio Graph',
  component: StudioGraph,
  parameters: {
    layout: 'fullscreen',
  },
} satisfies Meta<typeof StudioGraph>;

export default meta;
type Story = StoryObj<typeof meta>;

export const BroadcastMix: Story = {};

export const CapabilityInventory: Story = {
  render: () => <NodeInventory />,
};
