export type LicensingState = 'free' | 'licensed' | 'clock_invalid';

export interface LicensingStatus {
  status: LicensingState;
  expiresAtUnix: number | null;
  remainingDays: number;
  configured: boolean;
  hasActivation?: boolean;
}

/** The native desktop owns entitlement decisions and keeps the saved key. */
export interface LicensingPort {
  getStatus(): Promise<LicensingStatus>;
  activate(key: string, instanceName: string): Promise<LicensingStatus>;
  validate(): Promise<LicensingStatus>;
  deactivate(): Promise<LicensingStatus>;
  openCheckout(): Promise<void>;
}
