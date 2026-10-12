import { invoke } from '@tauri-apps/api/core';
import type { RuntimePort, RuntimeStatus } from './RuntimePort';

class TauriRuntimePort implements RuntimePort {
  connect(): Promise<RuntimeStatus> {
    return invoke<RuntimeStatus>('runtime_connect');
  }
  status(): Promise<RuntimeStatus> {
    return invoke<RuntimeStatus>('runtime_status');
  }
}

export const localRuntimePort: RuntimePort | null =
  '__TAURI_INTERNALS__' in window ? new TauriRuntimePort() : null;
