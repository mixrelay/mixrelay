import type { Meta, StoryObj } from '@storybook/react-vite';
import { fn } from 'storybook/test';
import { NativeEndpointBuffer } from '../../ui/NativeEndpointBuffer';
import { bufferControl } from './fixtures';

const meta = {
  title: 'Core/Playgrounds/NativeEndpointBuffer',
  component: NativeEndpointBuffer,
  argTypes: {
    onChange: { control: false },
  },
  args: { control: bufferControl, onChange: fn() },
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
          '선택은 요청만 기록하며 확인된 currentFrames를 바꾸지 않습니다. Controls의 control에서 readback 응답을 재현합니다.',
      },
    },
  },
} satisfies Meta<typeof NativeEndpointBuffer>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
export const Pending: Story = {
  args: { control: { ...bufferControl, currentFrames: null, preferredFrames: 256 } },
};
export const ReadbackMismatch: Story = {
  args: { control: { ...bufferControl, preferredFrames: 256 } },
};
export const Failed: Story = {
  args: { control: { ...bufferControl, error: '설정하지 못했습니다.' } },
};
export const Disabled: Story = { args: { control: { ...bufferControl, disabled: true } } };
