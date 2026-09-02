import type { Meta, StoryObj } from '@storybook/react-vite';

import { StatusBadge } from './StatusBadge';

const meta = {
  title: 'Core/Feedback/StatusBadge',
  component: StatusBadge,
  args: { children: 'Ready' },
} satisfies Meta<typeof StatusBadge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
export const Positive: Story = { args: { tone: 'positive' } };
export const Warning: Story = { args: { tone: 'warning', children: 'Restart required' } };
export const Critical: Story = { args: { tone: 'critical', children: 'Driver blocked' } };
