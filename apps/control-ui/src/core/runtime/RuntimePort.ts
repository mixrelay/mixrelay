export type RuntimeStatus = 'ready' | 'not_installed' | 'incompatible' | 'unavailable';

/** Audio belongs to MixRelay Driver; the editor explicitly connects before loading a workspace. */
export interface RuntimePort {
  connect(): Promise<RuntimeStatus>;
  /** Read-only: observing a stopped runtime must never restart audio. */
  status(): Promise<RuntimeStatus>;
}
