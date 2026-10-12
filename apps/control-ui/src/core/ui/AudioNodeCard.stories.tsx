import type { Meta, StoryObj } from '@storybook/react-vite';

import mixrelayAppIcon from '../../../../desktop/src-tauri/icons/32x32.png';
import { AudioNodeCard } from './AudioNodeCard';

const meta = {
  title: 'Core/Audio Nodes/Application Icon',
  component: AudioNodeCard,
  args: {
    category: 'Application',
    icon: 'APP',
    iconUrl: mixrelayAppIcon,
    title: 'MixRelay',
    inputCount: 0,
    outputCount: 2,
    onSelect: () => {},
  },
  decorators: [
    (Story) => (
      <div style={{ width: 'var(--node-default-width)' }}>
        <Story />
      </div>
    ),
  ],
} satisfies Meta<typeof AudioNodeCard>;

export default meta;
type Story = StoryObj<typeof meta>;

/** Use the actual application asset, rather than drawing a replacement logo. */
export const ActualAppIcon: Story = {};
export const IconUnavailable: Story = { args: { iconUrl: undefined } };
export const IconLoadFailure: Story = { args: { iconUrl: 'data:image/png;base64,broken' } };

export const ParentOwnsSelection: Story = { args: { onSelect: undefined } };
