import { useEffect, useState } from 'react';
import type {
  ControlPort,
  DeviceAppActivity,
  VirtualEndpointSettings,
} from '../../core/control-port/types';
import { structuralShare } from '../../core/structuralShare';

export type DeviceActivityObservation =
  | { state: 'loading' | 'unavailable' }
  | { state: 'observed'; devices: Record<string, DeviceAppActivity[]> };

/** One workspace reader. Identity fences retire replies across ports, UID mappings and visibility. */
export function useVirtualDeviceAppActivity(
  controlPort: ControlPort,
  settings?: VirtualEndpointSettings,
) {
  const mapping = JSON.stringify(
    settings?.devices.map(({ spec, nativeDeviceIds }) => [spec.id, nativeDeviceIds]),
  );
  const [snapshot, setSnapshot] = useState<{
    port: ControlPort;
    mapping: string | undefined;
    value: DeviceActivityObservation;
  } | null>(null);
  useEffect(() => {
    const read = controlPort.getVirtualDeviceAppActivity;
    if (!read) return;
    const publish = (value: DeviceActivityObservation) =>
      setSnapshot((previous) => {
        if (previous?.port === controlPort && previous.mapping === mapping) {
          const shared = structuralShare(previous.value, value);
          return shared === previous.value ? previous : { ...previous, value: shared };
        }
        return { port: controlPort, mapping, value };
      });
    return startPolling(controlPort, read, publish);
  }, [controlPort, mapping]);
  if (!controlPort.getVirtualDeviceAppActivity) return { state: 'unavailable' } as const;
  return snapshot?.port === controlPort && snapshot.mapping === mapping
    ? snapshot.value
    : ({ state: 'loading' } as const);
}

function startPolling(
  controlPort: ControlPort,
  read: NonNullable<ControlPort['getVirtualDeviceAppActivity']>,
  publish: (value: DeviceActivityObservation) => void,
) {
  let active = true;
  let generation = 0;
  let running = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let expiry: ReturnType<typeof setTimeout> | undefined;
  const expire = () => {
    clearTimeout(expiry);
    expiry = setTimeout(() => publish({ state: 'unavailable' }), 3000);
  };
  const poll = async () => {
    if (running || document.visibilityState === 'hidden') return;
    clearTimeout(timer);
    running = true;
    const request = generation;
    const current = () => active && request === generation;
    try {
      const devices = await read.call(controlPort);
      if (current()) {
        publish({ state: 'observed', devices });
        expire();
      }
    } catch {
      if (current()) publish({ state: 'unavailable' });
    } finally {
      if (current()) {
        running = false;
        timer = setTimeout(poll, 1000);
      }
    }
  };
  const refresh = () => {
    void poll();
  };
  const visibility = () => {
    generation += 1;
    running = false;
    clearTimeout(timer);
    clearTimeout(expiry);
    publish({ state: 'unavailable' });
    if (document.visibilityState !== 'hidden') {
      expire();
      refresh();
    }
  };
  publish({ state: 'loading' });
  expire();
  refresh();
  window.addEventListener('focus', refresh);
  document.addEventListener('visibilitychange', visibility);
  return () => {
    active = false;
    clearTimeout(timer);
    clearTimeout(expiry);
    window.removeEventListener('focus', refresh);
    document.removeEventListener('visibilitychange', visibility);
  };
}
