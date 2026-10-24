import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect } from 'react';
import { useTheme } from './MixRelayProvider';

/** Keeps the native window chrome on the chosen appearance; system follows the OS. */
export function NativeWindowTheme() {
  const { preference } = useTheme();
  useEffect(() => {
    if (!isTauri()) return;
    void getCurrentWindow()
      .setTheme(preference === 'system' ? null : preference)
      .catch((error: unknown) => {
        console.error('Could not update the native window appearance', error);
      });
  }, [preference]);
  return null;
}
