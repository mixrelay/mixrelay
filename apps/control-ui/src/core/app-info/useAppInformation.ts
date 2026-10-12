import { getVersion } from '@tauri-apps/api/app';
import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useEffect, useRef, useState } from 'react';
import { version as buildVersion } from '../../../package.json';

/** The native menu opens product information independently of the footer license workflow. */
export function useAppInformation(onOpen: () => void, enabled: boolean) {
  const [version, setVersion] = useState(buildVersion);
  const open = useRef(onOpen);
  open.current = onOpen;
  useEffect(() => {
    if (!enabled || !isTauri()) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    void getVersion()
      .then((value) => {
        if (!disposed) setVersion(value);
      })
      .catch((error: unknown) => console.error('Could not read app version', error));
    void listen('app-about', () => open.current())
      .then((unsubscribe) => {
        if (disposed) unsubscribe();
        else stop = unsubscribe;
      })
      .catch((error: unknown) => console.error('About menu listener failed', error));
    return () => {
      disposed = true;
      stop?.();
    };
  }, [enabled]);
  return version;
}
