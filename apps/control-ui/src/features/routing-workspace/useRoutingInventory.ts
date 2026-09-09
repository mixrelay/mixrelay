import { useCallback, useEffect, useRef, useState } from 'react';

import type {
  AudioDeviceDescriptor,
  CapturableApplication,
  ControlPort,
} from '../../core/control-port/types';

export const INVENTORY_POLL_MS = 5_000;

/** Discovery may include native icon extraction: only poll while a menu is visible. */
export function useRoutingInventory(
  controlPort: Pick<ControlPort, 'listCapturableApplications' | 'listAudioDevices'>,
  menuOpened: boolean,
) {
  const [applications, setApplications] = useState<CapturableApplication[]>([]);
  const [audioDevices, setAudioDevices] = useState<AudioDeviceDescriptor[]>([]);
  const [failed, setFailed] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const request = useRef<() => void>(() => undefined);
  const refresh = useCallback(() => request.current(), []);

  useEffect(() => {
    let cancelled = false;
    let running = false;
    setApplications([]);
    setAudioDevices([]);
    setFailed(false);
    const discover = () => {
      if (cancelled || running || document.visibilityState === 'hidden') return;
      running = true;
      setRefreshing(true);
      // A failed category must not erase its last successful list or prevent the other
      // category from updating. Native invoke has no cancellation; ignore late responses.
      void Promise.allSettled([
        Promise.resolve().then(() => controlPort.listCapturableApplications?.() ?? []),
        Promise.resolve().then(() => controlPort.listAudioDevices?.() ?? []),
      ]).then(([apps, devices]) => {
        running = false;
        if (cancelled) return;
        if (apps.status === 'fulfilled') setApplications(apps.value);
        if (devices.status === 'fulfilled') setAudioDevices(devices.value);
        setFailed(apps.status === 'rejected' || devices.status === 'rejected');
        setRefreshing(false);
      });
    };
    request.current = discover;
    discover();
    window.addEventListener('focus', discover);
    document.addEventListener('visibilitychange', discover);
    return () => {
      cancelled = true;
      request.current = () => undefined;
      window.removeEventListener('focus', discover);
      document.removeEventListener('visibilitychange', discover);
    };
  }, [controlPort]);

  useEffect(() => {
    if (!menuOpened) return;
    refresh();
    const timer = setInterval(refresh, INVENTORY_POLL_MS);
    return () => clearInterval(timer);
  }, [menuOpened, refresh]);

  return { applications, audioDevices, failed, refresh, refreshing };
}
