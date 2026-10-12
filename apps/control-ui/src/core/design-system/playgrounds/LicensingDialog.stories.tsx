import type { Meta, StoryObj } from '@storybook/react-vite';
import { fn } from 'storybook/test';
import { LicensingDialog } from '../../licensing/LicensingFields';
import { ActionButton } from '../../ui/ActionButton';

import { Text } from '../../ui/Text';
import { usePlaygroundState } from './usePlaygroundState';

const meta = {
  title: 'Core/Playgrounds/LicensingDialog',
  component: LicensingDialog,
  argTypes: {
    children: { control: false },
    onClose: { control: false },
  },
  args: {
    title: '인증 관리',
    opened: false,
    busy: false,
    closeLabel: '닫기',
    children: <Text>무료 인증 표본. 온라인 요청을 보내지 않습니다.</Text>,
    onClose: fn(),
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
  render: function Interactive(args) {
    const { local, set, commit } = usePlaygroundState(args);
    return (
      <fieldset
        aria-label="Playground 예제"
        onBlur={commit}
        style={{ border: 0, padding: 0, margin: 0, minWidth: 0 }}
      >
        <ActionButton onClick={() => set({ opened: true })}>인증 표본 열기</ActionButton>
        <LicensingDialog
          {...local}
          onClose={() => {
            local.onClose();
            set({ opened: false });
          }}
        />
      </fieldset>
    );
  },
} satisfies Meta<typeof LicensingDialog>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
export const Busy: Story = { args: { opened: true, busy: true } };
