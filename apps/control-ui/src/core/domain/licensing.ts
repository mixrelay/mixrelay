export type LicensingState = 'free' | 'licensed' | 'clock_invalid';

export interface LicensingStatus {
  status: LicensingState;
  expiresAtUnix: number | null;
  remainingDays: number;
  configured: boolean;
  hasActivation?: boolean;
}
