import { Channel, invoke } from '@tauri-apps/api/core';
import { literal } from '../adapters/decode';
import {
  isRuntimeConnectStage,
  type RuntimeConnectStage,
  type RuntimePort,
  type RuntimeStatus,
} from './RuntimePort';

const runtimeStatus = literal('ready', 'not_installed', 'incompatible', 'unavailable');

class TauriRuntimePort implements RuntimePort {
  async connect(onStage: (stage: RuntimeConnectStage) => void): Promise<RuntimeStatus> {
    // Stages are progress only; an unknown message never changes the connection outcome.
    const stages = new Channel<unknown>((message) => {
      if (isRuntimeConnectStage(message)) onStage(message);
    });
    return runtimeStatus(await invoke('runtime_connect', { onStage: stages }), 'runtime_connect');
  }
  async status(): Promise<RuntimeStatus> {
    return runtimeStatus(await invoke('runtime_status'), 'runtime_status');
  }
}

export const localRuntimePort: RuntimePort | null =
  '__TAURI_INTERNALS__' in window ? new TauriRuntimePort() : null;
