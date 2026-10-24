import { invoke } from '@tauri-apps/api/core';
import type { DriverCommand, DriverPort, DriverSnapshot } from './DriverPort';

export class TauriDriverPort implements DriverPort {
  getSnapshot(): Promise<DriverSnapshot> {
    return invoke<DriverSnapshot>('driver_status');
  }

  mutate(command: DriverCommand): Promise<DriverSnapshot> {
    return invoke<DriverSnapshot>('driver_mutate', { command });
  }
}

export const localDriverPort: DriverPort | null =
  '__TAURI_INTERNALS__' in window ? new TauriDriverPort() : null;
