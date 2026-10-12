import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  type ControlPort,
  ControlPortError,
  type MixRelayStatus,
} from '../../core/control-port/types';

/** Device settings are owned by the Driver runtime, never by the graph document. */
export function useDeviceBufferControl(
  controlPort: ControlPort,
  status: MixRelayStatus | null,
  onBusyChange: (busy: boolean) => void,
  onApplied: () => void,
  disabled = false,
) {
  const { t } = useTranslation();
  const [error, setError] = useState<{ endpointId: string; message: string } | null>(null);
  const owner = useRef(controlPort);
  owner.current = controlPort;
  const generation = useRef(0);
  const releaseBusy = useRef<(() => void) | null>(null);
  useEffect(() => {
    owner.current = controlPort;
    setError(null);
    return () => {
      generation.current += 1;
      releaseLease(releaseBusy);
    };
  }, [controlPort]);
  const change = async (endpointId: string, bufferFrames: number) => {
    const request = deviceBufferRequest(status, endpointId, bufferFrames);
    if (disabled || !request || !controlPort.setVirtualEndpointBufferFrames || releaseBusy.current)
      return;
    const ticket = generation.current;
    const current = () => owner.current === controlPort && ticket === generation.current;
    releaseBusy.current = () => onBusyChange(false);
    onBusyChange(true);
    setError(null);
    try {
      const graph = await controlPort.getGraph();
      if (!current()) return;
      const result = await controlPort.setVirtualEndpointBufferFrames({
        ...request,
        expectedRevision: graph.revision,
      });
      if (!current()) return;
      setError(
        result.persistenceError
          ? {
              endpointId,
              message: t('audioSession.issues.endpoint_preferences_unavailable'),
            }
          : null,
      );
      onApplied();
    } catch (cause) {
      if (!current()) return;
      setError({
        endpointId,
        message: t(bufferErrorKey(cause), {
          defaultValue: t('audioSession.changeFailed'),
        }),
      });
      // Re-read the actual setting after failure or an external edit; never assume rollback.
      onApplied();
    } finally {
      releaseLease(releaseBusy, current());
    }
  };
  return {
    enabled: !disabled && Boolean(controlPort.setVirtualEndpointBufferFrames),
    error,
    change: (endpointId: string, frames: number) => {
      void change(endpointId, frames);
    },
  };
}

function releaseLease(lease: { current: (() => void) | null }, allowed = true) {
  if (!allowed) return;
  const release = lease.current;
  lease.current = null;
  release?.();
}

function bufferErrorKey(cause: unknown) {
  return cause instanceof ControlPortError
    ? `routing.errors.${cause.body.code}`
    : 'audioSession.changeFailed';
}

function deviceBufferRequest(
  status: MixRelayStatus | null,
  endpointId: string,
  bufferFrames: number,
) {
  const buffer = status?.audioSession?.virtualEndpointSettings?.devices.find(
    (device) => device.spec.id === endpointId,
  )?.nativeBuffer;
  if (
    !buffer?.canSet ||
    !Number.isInteger(bufferFrames) ||
    bufferFrames < buffer.minimumFrames ||
    bufferFrames > buffer.maximumFrames
  )
    return null;
  return {
    endpointId,
    expectedBufferFrames: buffer.currentFrames,
    expectedPreferredFrames: buffer.preferredFrames,
    bufferFrames,
  };
}
