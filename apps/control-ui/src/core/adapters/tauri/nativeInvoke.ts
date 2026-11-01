import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/** A native command. Replies are untyped until an adapter decodes them. */
export type NativeInvoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

/** Registers a payload-free native event listener; resolves to its removal. */
export type NativeListen = (event: string, listener: () => void) => Promise<() => void>;

export const tauriInvoke: NativeInvoke = (command, ...args) => invoke(command, ...args);

export const tauriListen: NativeListen = (event, listener) => listen(event, () => listener());
