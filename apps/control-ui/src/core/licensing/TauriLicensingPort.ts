import { invoke } from '@tauri-apps/api/core';
import { boolean, integer, literal, nullable, object, optional } from '../adapters/decode';
import type { LicensingPort, LicensingStatus } from './LicensingPort';

const licensingStatus = object<LicensingStatus>({
  status: literal('free', 'licensed', 'clock_invalid'),
  expiresAtUnix: nullable(integer),
  remainingDays: integer,
  configured: boolean,
  hasActivation: optional(boolean),
});

async function status(command: string, ...args: [Record<string, unknown>?]) {
  return licensingStatus(await invoke(command, ...args), command);
}

export class TauriLicensingPort implements LicensingPort {
  getStatus(): Promise<LicensingStatus> {
    return status('licensing_status');
  }

  activate(key: string, instanceName: string): Promise<LicensingStatus> {
    return status('licensing_activate', { key, instanceName });
  }

  validate(): Promise<LicensingStatus> {
    return status('licensing_validate');
  }

  deactivate(): Promise<LicensingStatus> {
    return status('licensing_deactivate');
  }

  async openCheckout(): Promise<void> {
    await invoke('licensing_open_checkout');
  }
}

/** Browser preview and LAN clients do not create or assert a local trial. */
export const localLicensingPort: LicensingPort | null =
  '__TAURI_INTERNALS__' in window ? new TauriLicensingPort() : null;
