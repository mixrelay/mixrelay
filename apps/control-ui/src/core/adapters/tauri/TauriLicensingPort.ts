import type { LicensingStatus } from '../../domain/licensing';
import type { LicensingPort } from '../../ports/LicensingPort';
import { boolean, literal, nonNegativeInteger, nullable, object, optional } from '../decode';
import { type NativeInvoke, tauriInvoke } from './nativeInvoke';

const licensingStatus = object<LicensingStatus>({
  status: literal('free', 'licensed', 'clock_invalid'),
  expiresAtUnix: nullable(nonNegativeInteger),
  remainingDays: nonNegativeInteger,
  configured: boolean,
  hasActivation: optional(boolean),
});

export class TauriLicensingPort implements LicensingPort {
  constructor(private readonly invoke: NativeInvoke = tauriInvoke) {}

  getStatus(): Promise<LicensingStatus> {
    return this.status('licensing_status');
  }

  activate(key: string, instanceName: string): Promise<LicensingStatus> {
    return this.status('licensing_activate', { key, instanceName });
  }

  validate(): Promise<LicensingStatus> {
    return this.status('licensing_validate');
  }

  deactivate(): Promise<LicensingStatus> {
    return this.status('licensing_deactivate');
  }

  async openCheckout(): Promise<void> {
    await this.invoke('licensing_open_checkout');
  }

  private async status(command: string, ...args: [Record<string, unknown>?]) {
    return licensingStatus(await this.invoke(command, ...args), command);
  }
}
