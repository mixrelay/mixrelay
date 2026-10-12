import type { Meta, StoryObj } from '@storybook/react-vite';
import { ProgressIndicator } from '../../ui/ProgressIndicator';

const meta = {
  title: 'Core/Playgrounds/ProgressIndicator',
  component: ProgressIndicator,
  args: { label: 'MixRelay에 연결하는 중' },
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
          '진행률을 알 수 없는 대기를 표시합니다. 링은 장식이고 label이 status로 한 번 읽힙니다. reduced motion에서는 회전하지 않습니다.',
      },
    },
  },
} satisfies Meta<typeof ProgressIndicator>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
export const LongLabel: Story = {
  args: { label: 'MixRelay 오디오 엔진과 드라이버의 준비 상태를 확인하는 중입니다' },
};
