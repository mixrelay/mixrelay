import type { Meta, StoryObj } from '@storybook/react-vite';
import { fn } from 'storybook/test';
import appIcon from '../../../../../desktop/src-tauri/icons/32x32.png';
import { AudioNodeCard } from '../../ui/AudioNodeCard';

const meta = {
  title: 'Core/Playgrounds/AudioNodeCard',
  component: AudioNodeCard,
  argTypes: {
    onSelect: { control: false },
  },
  args: {
    category: 'Application',
    icon: 'APP',
    iconUrl: appIcon,
    title: 'MixRelay',
    inputCount: 0,
    outputCount: 2,
    channelCount: 2,
    poweredOff: false,
    bypassed: false,
    offline: false,
    selected: false,
    onSelect: fn(),
  },
  parameters: {
    controls: {
      exclude: [
        'className',
        'classNames',
        'style',
        'styles',
        'vars',
        'unstyled',
        'color',
        'c',
        'size',
        'radius',
        'variant',
        'component',
        'renderRoot',
        'dangerouslySetInnerHTML',
      ],
    },
    docs: {
      source: { type: 'dynamic' },
      description: {
        component:
          'Controls는 실제 공개 props를 전달합니다. 샘플 callback은 Actions에 기록하며 제품 요청을 실행하지 않습니다.',
      },
    },
  },
  render: (args) => (
    <div style={{ width: 'var(--node-default-width)', maxWidth: '100%' }}>
      <AudioNodeCard {...args} />
    </div>
  ),
} satisfies Meta<typeof AudioNodeCard>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
export const Off: Story = { args: { poweredOff: true } };
export const Bypassed: Story = { args: { bypassed: true } };
export const Offline: Story = { args: { offline: true } };
export const Selected: Story = { args: { selected: true } };
export const Fallback: Story = { args: { iconUrl: undefined } };
