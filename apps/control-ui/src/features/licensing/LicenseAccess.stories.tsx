import type { Meta, StoryObj } from '@storybook/react-vite';
import { userEvent, within } from 'storybook/test';

import type { LicensingPort, LicensingStatus } from '../../core/licensing/LicensingPort';
import { LicenseAccess } from './LicenseAccess';

const trial: LicensingStatus = {
  status: 'trial',
  expiresAtUnix: 1_900_000_000,
  remainingDays: 8,
  configured: true,
};
const licensed: LicensingStatus = { ...trial, status: 'licensed', remainingDays: 27 };
const expired: LicensingStatus = {
  ...trial,
  status: 'expired',
  expiresAtUnix: 1_600_000_000,
  remainingDays: 0,
};

function fixturePort(status: LicensingStatus): LicensingPort {
  return {
    getStatus: async () => status,
    activate: async () => licensed,
    validate: async () => licensed,
    deactivate: async () => expired,
    openCheckout: async () => {},
  };
}

const preview = <div style={{ minHeight: '100vh', padding: 24 }}>Storybook workspace preview</div>;

const meta = {
  title: 'Features/Licensing/MockedLicenseAccess',
  component: LicenseAccess,
  args: { children: preview, port: fixturePort(trial) },
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: { component: 'Deterministic UI fixtures only; no native license is checked.' },
    },
  },
} satisfies Meta<typeof LicenseAccess>;

export default meta;
type Story = StoryObj<typeof meta>;

export const TrialCompact: Story = {};

export const ExpiredActivation: Story = {
  args: { port: fixturePort(expired), onStopAudio: async () => {} },
};

export const LicensedDialog: Story = {
  args: { port: fixturePort(licensed) },
  play: async ({ canvasElement }) => {
    const button = await within(canvasElement).findByRole('button', {
      name: /Licensed|라이선스 사용 중|Lïcénśéd/u,
    });
    await userEvent.click(button);
  },
};

export const LocalStorageError: Story = {
  args: {
    port: {
      ...fixturePort(trial),
      getStatus: async () => {
        throw new Error('storage_unavailable');
      },
    },
    onStopAudio: async () => {},
  },
};
