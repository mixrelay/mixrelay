import { invoke } from '@tauri-apps/api/core';

import type { LicensingPort, LicensingStatus } from './LicensingPort';

export class TauriLicensingPort implements LicensingPort {
  getStatus(): Promise<LicensingStatus> {
    return invoke<LicensingStatus>('licensing_status');
  }

  activate(key: string, instanceName: string): Promise<LicensingStatus> {
    return invoke<LicensingStatus>('licensing_activate', { key, instanceName });
  }

  validate(): Promise<LicensingStatus> {
    return invoke<LicensingStatus>('licensing_validate');
  }

  deactivate(): Promise<LicensingStatus> {
    return invoke<LicensingStatus>('licensing_deactivate');
  }

  openCheckout(): Promise<void> {
    return invoke<void>('licensing_open_checkout');
  }
}

/** Browser preview and LAN clients do not create or assert a local trial. */
export const localLicensingPort: LicensingPort | null =
  '__TAURI_INTERNALS__' in window ? new TauriLicensingPort() : null;
