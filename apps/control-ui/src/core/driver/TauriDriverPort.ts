import { invoke } from '@tauri-apps/api/core';
import { virtualEndpointSettings } from '../adapters/control/protocolDecoders';
import { integer, literal, nullable, object, optional, string } from '../adapters/decode';
import type { DriverCommand, DriverPort, DriverSnapshot } from './DriverPort';

const driverSnapshot = object<DriverSnapshot>({
  protocolVersion: literal(1),
  revision: integer,
  state: literal('ready', 'not_installed', 'installed_not_loaded', 'unavailable'),
  installedVersion: nullable(string),
  settings: virtualEndpointSettings,
  persistenceError: optional(string),
});

export class TauriDriverPort implements DriverPort {
  async getSnapshot(): Promise<DriverSnapshot> {
    return driverSnapshot(await invoke('driver_status'), 'driver_status');
  }

  async mutate(command: DriverCommand): Promise<DriverSnapshot> {
    return driverSnapshot(await invoke('driver_mutate', { command }), 'driver_mutate');
  }
}

export const localDriverPort: DriverPort | null =
  '__TAURI_INTERNALS__' in window ? new TauriDriverPort() : null;
