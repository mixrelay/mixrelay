import { Channel, invoke } from '@tauri-apps/api/core';
import {
  isRuntimeConnectStage,
  type RuntimeConnectStage,
  type RuntimePort,
  type RuntimeStatus,
} from './RuntimePort';

class TauriRuntimePort implements RuntimePort {
  connect(onStage: (stage: RuntimeConnectStage) => void): Promise<RuntimeStatus> {
    // Stages are progress only; an unknown message never changes the connection outcome.
    const stages = new Channel<unknown>((message) => {
      if (isRuntimeConnectStage(message)) onStage(message);
    });
    return invoke<RuntimeStatus>('runtime_connect', { onStage: stages });
  }
  status(): Promise<RuntimeStatus> {
    return invoke<RuntimeStatus>('runtime_status');
  }
}

export const localRuntimePort: RuntimePort | null =
  '__TAURI_INTERNALS__' in window ? new TauriRuntimePort() : null;
