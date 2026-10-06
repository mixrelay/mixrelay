import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DriverAction, DriverPort, DriverSnapshot } from '../../core/driver/DriverPort';
import type { EndpointUpdate } from '../../core/ui/AudioNodeCard';

interface Owner {
  port: DriverPort;
  active: boolean;
  busy: boolean;
  snapshot: DriverSnapshot | null;
}

export function useDriverSession(port: DriverPort | null, allowMultiDevice = false) {
  const { t } = useTranslation();
  const owner = useRef<Owner | null>(null);
  const currentPort = useRef(port);
  currentPort.current = port;
  const [state, setState] = useState<{
    port: DriverPort | null;
    snapshot: DriverSnapshot | null;
    busy: boolean;
    error: string | null;
    endpointId: string | null;
  }>({ port, snapshot: null, busy: false, error: null, endpointId: null });

  const refresh = useCallback(async () => {
    const current = owner.current;
    if (!current?.active || current.port !== port || current.busy) return;
    current.busy = true;
    setState((value) => ({ ...value, busy: true }));
    try {
      const snapshot = await current.port.getSnapshot();
      if (!current.active || currentPort.current !== current.port) return;
      current.snapshot = snapshot;
      setState((value) => ({ ...value, snapshot, error: value.endpointId ? value.error : null }));
    } catch {
      if (current.active && currentPort.current === current.port) {
        current.snapshot = null;
        setState((value) => ({ ...value, snapshot: null, error: t('driverManagement.unavailable') }));
      }
    } finally {
      current.busy = false;
      if (current.active && currentPort.current === current.port)
        setState((value) => ({ ...value, busy: false }));
    }
  }, [port, t]);

  useEffect(() => {
    setState({ port, snapshot: null, busy: false, error: null, endpointId: null });
    if (!port) { owner.current = null; return; }
    const current: Owner = { port, active: true, busy: false, snapshot: null };
    owner.current = current;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      await refresh();
      if (current.active) timer = setTimeout(() => void poll(), 5_000);
    };
    void Promise.resolve().then(() => { if (current.active) void poll(); });
    return () => { current.active = false; clearTimeout(timer); };
  }, [port, refresh]);

  const mutate = useCallback(async (action: DriverAction): Promise<DriverSnapshot | null> => {
    const current = owner.current;
    if (!current?.active || current.port !== port || current.busy || current.snapshot?.state !== 'ready') return null;
    if ((action.action === 'create' || action.action === 'remove') && !allowMultiDevice) {
      setState((value) => ({ ...value, error: t('driverManagement.paidRequired') }));
      return null;
    }
    const endpointId = 'endpointId' in action ? action.endpointId : action.spec.id;
    if (action.action !== 'create' && !current.snapshot.settings.devices.some((device) => device.spec.id === endpointId)) {
      setState((value) => ({ ...value, error: t('driverManagement.deviceMissing'), endpointId }));
      return null;
    }
    current.busy = true;
    setState((value) => ({ ...value, busy: true, error: null, endpointId }));
    try {
      const snapshot = await current.port.mutate({ ...action, expectedRevision: current.snapshot.revision });
      if (!current.active || currentPort.current !== current.port) return null;
      current.snapshot = snapshot;
      setState((value) => ({ ...value, snapshot, error: snapshot.persistenceError ? t('driverManagement.saveFailed') : null }));
      return snapshot;
    } catch {
      if (current.active && currentPort.current === current.port)
        setState((value) => ({ ...value, error: t('driverManagement.changeFailed') }));
      return null;
    } finally {
      current.busy = false;
      if (current.active && currentPort.current === current.port)
        setState((value) => ({ ...value, busy: false }));
    }
  }, [port, t, allowMultiDevice]);

  const changeDevice = useCallback(async (endpointId: string, update: EndpointUpdate) => {
    const device = owner.current?.snapshot?.settings.devices.find((candidate) => candidate.spec.id === endpointId);
    if (!device) {
      setState((value) => ({ ...value, error: t('driverManagement.deviceMissing'), endpointId }));
      return null;
    }
    if (update.nativeFormat) return mutate({ action: 'set_format', endpointId, ...update.nativeFormat });
    const spec = device.spec;
    return mutate({ action: 'configure', spec: {
      ...spec,
      displayName: update.displayName,
      defaultSampleRate: update.sampleRate ?? spec.defaultSampleRate,
      render: spec.render && update.renderChannels !== spec.render.channels
        ? { channels: update.renderChannels, channelLayout: update.renderChannels === 1 ? 'mono' : update.renderChannels === 2 ? 'stereo' : 'discrete' } : spec.render,
      capture: spec.capture && update.captureChannels !== spec.capture.channels
        ? { channels: update.captureChannels, channelLayout: update.captureChannels === 1 ? 'mono' : update.captureChannels === 2 ? 'stereo' : 'discrete' } : spec.capture,
    } });
  }, [mutate, t]);

  const value = state.port === port ? state : { snapshot: null, busy: false, error: null, endpointId: null };
  return { ...value, allowMultiDevice, refresh, mutate, changeDevice };
}

export type DriverSession = ReturnType<typeof useDriverSession>;
