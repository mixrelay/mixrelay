import type { Meta, StoryObj } from '@storybook/react-vite';
import { expect, userEvent, waitFor, within } from 'storybook/test';
import type { LicensingStatus } from '../../core/domain/licensing';
import { i18n } from '../../core/i18n';
import type { LicensingPort } from '../../core/ports/LicensingPort';
import { LicenseAccess } from './LicenseAccess';

const free: LicensingStatus = {
  status: 'free',
  expiresAtUnix: null,
  remainingDays: 0,
  configured: true,
};
const licensed: LicensingStatus = {
  ...free,
  status: 'licensed',
  expiresAtUnix: 1_900_000_000,
  remainingDays: 27,
  hasActivation: true,
};
const savedSeat: LicensingStatus = {
  ...free,
  status: 'free',
  expiresAtUnix: null,
  hasActivation: true,
  remainingDays: 0,
};

function fixturePort(status: LicensingStatus): LicensingPort {
  return {
    getStatus: async () => status,
    activate: async () => licensed,
    validate: async () => licensed,
    deactivate: async () => free,
    openCheckout: async () => {},
  };
}

const preview = <div style={{ minHeight: '100vh', padding: 24 }}>Storybook workspace preview</div>;

const meta = {
  title: 'Features/Licensing/MockedLicenseAccess',
  component: LicenseAccess,
  args: { children: preview, port: fixturePort(free) },
  parameters: {
    layout: 'fullscreen',
    docs: {
      description: { component: 'Deterministic UI fixtures only; no native license is checked.' },
    },
  },
} satisfies Meta<typeof LicenseAccess>;

export default meta;
type Story = StoryObj<typeof meta>;

export const FreeCompact: Story = {};

export const ExpiredPaidSeat: Story = {
  args: { port: fixturePort(savedSeat) },
};

export const LicensedDialog: Story = {
  args: { port: fixturePort(licensed) },
  play: async ({ canvasElement }) => {
    const button = await within(canvasElement).findByRole('button', {
      name: i18n.t('licensing.states.licensed', { count: licensed.remainingDays }),
    });
    await userEvent.click(button);
    const dialog = await within(canvasElement.ownerDocument.body).findByRole('dialog', {
      name: i18n.t('licensing.title'),
    });
    await waitFor(() => expect(dialog).toBeVisible());
  },
};

export const LocalStorageError: Story = {
  args: {
    port: {
      ...fixturePort(free),
      getStatus: async () => {
        throw new Error('storage_unavailable');
      },
    },
  },
};

export const FooterLicense: Story = {
  args: { port: fixturePort(licensed), presentation: 'footer' },
  play: async ({ canvasElement }) => {
    await userEvent.click(
      await within(canvasElement).findByRole('button', {
        name: i18n.t('licensing.title'),
      }),
    );
    const dialog = await within(canvasElement.ownerDocument.body).findByRole('dialog', {
      name: i18n.t('licensing.title'),
    });
    await waitFor(() => expect(dialog).toBeVisible());
  },
};
