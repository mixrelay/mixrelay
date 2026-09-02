export type DriverState =
  | 'not_installed'
  | 'installing'
  | 'restart_required'
  | 'installed_not_loaded'
  | 'blocked'
  | 'invalid_permissions'
  | 'version_mismatch'
  | 'ready';

export interface ToneNariStatus {
  protocolVersion: 1;
  driverState: DriverState;
  sampleRate: 44_100 | 48_000;
  bufferFrames: number;
  underruns: number;
  overruns: number;
}

export interface ControlPort {
  getStatus(): Promise<ToneNariStatus>;
}
