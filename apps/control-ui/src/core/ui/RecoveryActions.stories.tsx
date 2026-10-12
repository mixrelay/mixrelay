import type { Meta, StoryObj } from '@storybook/react-vite';
import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ActionButton } from './ActionButton';
import { RecoveryActions } from './RecoveryActions';
import { Stack } from './Stack';
import { TitleField } from './TitleField';
import { useRemovedFocusReturn } from './useRemovedFocusReturn';

function RecoveryExample() {
  const { t } = useTranslation();
  const [blocked, setBlocked] = useState(true);
  const title = useRef<HTMLInputElement>(null);
  const focusReturn = useRemovedFocusReturn(title);
  return (
    <Stack>
      <TitleField
        ref={title}
        aria-label={t('studio.graphName')}
        defaultValue={t('studio.untitledGraph')}
      />
      {blocked ? (
        <div role="alert" {...focusReturn}>
          {t('routing.recovery.draftPreserved')}
          <RecoveryActions
            retryLabel={t('routing.recovery.retry')}
            reloadLabel={t('routing.recovery.reload')}
            onRetry={() => setBlocked(false)}
            onReload={() => setBlocked(false)}
          />
        </div>
      ) : (
        <ActionButton onClick={() => setBlocked(true)}>{t('routing.recovery.retry')}</ActionButton>
      )}
    </Stack>
  );
}

const meta = { title: 'Core/Feedback/Recovery Actions', component: RecoveryExample } satisfies Meta<
  typeof RecoveryExample
>;
export default meta;
type Story = StoryObj<typeof meta>;
export const RemovedFocusReturn: Story = {};
export const NarrowLongTranslation: Story = {
  globals: { locale: 'pseudo', theme: 'dark' },
  decorators: [
    (Story) => (
      <div style={{ width: 280 }}>
        <Story />
      </div>
    ),
  ],
};
