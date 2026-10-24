import { HttpControlPort } from './HttpControlPort';
import { TauriControlPort } from './TauriControlPort';
import type { ControlPort } from './types';

export function createLocalControlPort(isTauri: boolean): ControlPort {
  return isTauri ? new TauriControlPort() : new HttpControlPort();
}

export const localControlPort = createLocalControlPort('__TAURI_INTERNALS__' in window);
