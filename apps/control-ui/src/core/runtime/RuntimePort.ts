export type RuntimeStatus = 'ready' | 'not_installed' | 'incompatible' | 'unavailable';

/** Steps of an explicit connection, in order. A Driver that is already running skips `starting`. */
export const RUNTIME_CONNECT_STAGES = ['checking', 'starting', 'preparing_engine'] as const;
export type RuntimeConnectStage = (typeof RUNTIME_CONNECT_STAGES)[number];

export function isRuntimeConnectStage(value: unknown): value is RuntimeConnectStage {
  return RUNTIME_CONNECT_STAGES.some((stage) => stage === value);
}

/** Audio belongs to MixRelay Driver; the editor explicitly connects before loading a workspace. */
export interface RuntimePort {
  /** Reports each step as the native side reaches it, then resolves with the outcome. */
  connect(onStage: (stage: RuntimeConnectStage) => void): Promise<RuntimeStatus>;
  /** Read-only: observing a stopped runtime must never restart audio. */
  status(): Promise<RuntimeStatus>;
}
