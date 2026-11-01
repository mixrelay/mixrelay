import {
  type Dispatch,
  type RefObject,
  type SetStateAction,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import type { VirtualEndpointSpec } from '../../core/domain/control/status';
import type { DriverAction, DriverPort, DriverSnapshot } from '../../core/driver/DriverPort';
import type { EndpointUpdate } from '../../core/ui/AudioNodeEndpointOptions';

interface Owner {
  port: DriverPort;
  active: boolean;
  busy: boolean;
  snapshot: DriverSnapshot | null;
}

interface DriverState {
  port: DriverPort | null;
  snapshot: DriverSnapshot | null;
  busy: boolean;
  error: string | null;
  endpointId: string | null;
}

interface SessionContext {
  port: DriverPort | null;
  owner: RefObject<Owner | null>;
  currentPort: RefObject<DriverPort | null>;
  setState: Dispatch<SetStateAction<DriverState>>;
  t: ReturnType<typeof useTranslation>['t'];
}

function ownsSession(current: Owner, currentPort: RefObject<DriverPort | null>) {
  return current.active && currentPort.current === current.port;
}

function requiresPaidDeviceControl(action: DriverAction, snapshot: DriverSnapshot) {
  if (action.action === 'create' || action.action === 'remove') return true;
  return (
    action.action === 'configure' &&
    snapshot.settings.devices.some(
      (device) =>
        device.spec.id === action.spec.id && device.spec.displayName !== action.spec.displayName,
    )
  );
}

function resizedChannels(stream: VirtualEndpointSpec['render'], channels: number) {
  if (!stream || channels === stream.channels) return stream;
  return {
    channels,
    channelLayout:
      channels === 1
        ? ('mono' as const)
        : channels === 2
          ? ('stereo' as const)
          : ('discrete' as const),
  };
}

function configuredDevice(spec: VirtualEndpointSpec, update: EndpointUpdate): DriverAction {
  return {
    action: 'configure',
    spec: {
      ...spec,
      displayName: update.displayName,
      defaultSampleRate: update.sampleRate ?? spec.defaultSampleRate,
      render: resizedChannels(spec.render, update.renderChannels),
      capture: resizedChannels(spec.capture, update.captureChannels),
    },
  };
}

function useDriverRefresh({ port, owner, currentPort, setState, t }: SessionContext) {
  return useCallback(async () => {
    const current = owner.current;
    if (!current?.active || current.port !== port || current.busy) return;
    current.busy = true;
    setState((value) => ({ ...value, busy: true }));
    try {
      const snapshot = await current.port.getSnapshot();
      if (!ownsSession(current, currentPort)) return;
      current.snapshot = snapshot;
      setState((value) => ({ ...value, snapshot, error: value.endpointId ? value.error : null }));
    } catch {
      if (ownsSession(current, currentPort)) {
        current.snapshot = null;
        setState((value) => ({
          ...value,
          snapshot: null,
          error: t('driverManagement.unavailable'),
        }));
      }
    } finally {
      current.busy = false;
      if (ownsSession(current, currentPort)) setState((value) => ({ ...value, busy: false }));
    }
  }, [port, t, owner, currentPort, setState]);
}

function mutationTarget(
  current: Owner | null,
  port: DriverPort | null,
  action: DriverAction,
  allowMultiDevice: boolean,
  setState: SessionContext['setState'],
  t: SessionContext['t'],
): { current: Owner; snapshot: DriverSnapshot; endpointId: string } | null {
  if (
    !current?.active ||
    current.port !== port ||
    current.busy ||
    current.snapshot?.state !== 'ready'
  )
    return null;
  if (!allowMultiDevice && requiresPaidDeviceControl(action, current.snapshot)) {
    setState((value) => ({ ...value, error: t('driverManagement.paidRequired') }));
    return null;
  }
  const endpointId = 'endpointId' in action ? action.endpointId : action.spec.id;
  if (
    action.action !== 'create' &&
    !current.snapshot.settings.devices.some((device) => device.spec.id === endpointId)
  ) {
    setState((value) => ({ ...value, error: t('driverManagement.deviceMissing'), endpointId }));
    return null;
  }
  return { current, snapshot: current.snapshot, endpointId };
}

function useDriverMutation(
  { port, owner, currentPort, setState, t }: SessionContext,
  allowMultiDevice: boolean,
) {
  return useCallback(
    async (action: DriverAction): Promise<DriverSnapshot | null> => {
      const target = mutationTarget(owner.current, port, action, allowMultiDevice, setState, t);
      if (!target) return null;
      const { current, snapshot: previous, endpointId } = target;
      current.busy = true;
      setState((value) => ({ ...value, busy: true, error: null, endpointId }));
      try {
        const snapshot = await current.port.mutate({
          ...action,
          expectedRevision: previous.revision,
        });
        if (!ownsSession(current, currentPort)) return null;
        current.snapshot = snapshot;
        setState((value) => ({
          ...value,
          snapshot,
          error: snapshot.persistenceError ? t('driverManagement.saveFailed') : null,
        }));
        return snapshot;
      } catch {
        if (ownsSession(current, currentPort))
          setState((value) => ({ ...value, error: t('driverManagement.changeFailed') }));
        return null;
      } finally {
        current.busy = false;
        if (ownsSession(current, currentPort)) setState((value) => ({ ...value, busy: false }));
      }
    },
    [port, t, allowMultiDevice, owner, currentPort, setState],
  );
}

function useDriverDeviceChange(
  { owner, setState, t }: SessionContext,
  mutate: ReturnType<typeof useDriverMutation>,
) {
  return useCallback(
    async (endpointId: string, update: EndpointUpdate) => {
      const device = owner.current?.snapshot?.settings.devices.find(
        (candidate) => candidate.spec.id === endpointId,
      );
      if (!device) {
        setState((value) => ({ ...value, error: t('driverManagement.deviceMissing'), endpointId }));
        return null;
      }
      if (update.sampleFormat)
        return mutate({
          action: 'set_sample_format',
          endpointId,
          sampleFormat: update.sampleFormat,
        });
      return mutate(configuredDevice(device.spec, update));
    },
    [mutate, t, owner, setState],
  );
}

export function useDriverSession(port: DriverPort | null, allowMultiDevice = false) {
  const { t } = useTranslation();
  const owner = useRef<Owner | null>(null);
  const currentPort = useRef(port);
  currentPort.current = port;
  const [state, setState] = useState<DriverState>({
    port,
    snapshot: null,
    busy: false,
    error: null,
    endpointId: null,
  });
  const context = { port, owner, currentPort, setState, t };
  const refresh = useDriverRefresh(context);
  const mutate = useDriverMutation(context, allowMultiDevice);
  const changeDevice = useDriverDeviceChange(context, mutate);
  useEffect(() => {
    setState({ port, snapshot: null, busy: false, error: null, endpointId: null });
    if (!port) {
      owner.current = null;
      return;
    }
    const current: Owner = { port, active: true, busy: false, snapshot: null };
    owner.current = current;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      await refresh();
      if (current.active) timer = setTimeout(() => void poll(), 5_000);
    };
    void Promise.resolve().then(() => {
      if (current.active) void poll();
    });
    return () => {
      current.active = false;
      clearTimeout(timer);
    };
  }, [port, refresh]);

  const value =
    state.port === port ? state : { snapshot: null, busy: false, error: null, endpointId: null };
  return { ...value, allowMultiDevice, refresh, mutate, changeDevice };
}

export type DriverSession = ReturnType<typeof useDriverSession>;
