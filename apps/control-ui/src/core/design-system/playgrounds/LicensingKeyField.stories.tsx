import type { Meta, StoryObj } from '@storybook/react-vite';
import { fn } from 'storybook/test';
import { LicensingKeyField } from '../../licensing/LicensingFields';
import { usePlaygroundState } from './usePlaygroundState';

const meta = {
  title: 'Core/Playgrounds/LicensingKeyField',
  component: LicensingKeyField,
  argTypes: {
    onChange: { control: false },
  },
  args: { label: '샘플 라이선스 키', value: 'example-only', disabled: false, onChange: fn() },
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
        <LicensingKeyField
          {...local}
          onChange={(value) => {
            local.onChange(value);
            set({ value });
          }}
        />
      </fieldset>
    );
  },
} satisfies Meta<typeof LicensingKeyField>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Playground: Story = {};
export const Disabled: Story = { args: { disabled: true } };
export const Empty: Story = { args: { value: '' } };
