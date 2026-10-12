import type { Meta, StoryObj } from '@storybook/react-vite';
import { StatusIndicator } from '../../ui/StatusIndicator';
import styles from './Playground.module.css';

const meta = {
  title: 'Core/Playgrounds/StatusIndicator',
  component: StatusIndicator,
  argTypes: {
    className: { control: false },
  },
  args: { label: '확인 완료', tone: 'positive', className: styles.indicator },
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
          'className은 실제 공개 slot입니다. 이 예시는 역할 토큰을 소비하는 샘플 marker 배치를 제공합니다.',
      },
    },
  },
} satisfies Meta<typeof StatusIndicator>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
export const Critical: Story = { args: { label: '조회 실패', tone: 'critical' } };
export const Warning: Story = { args: { label: '확인 필요', tone: 'warning' } };
