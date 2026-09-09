import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  type ControlPort,
  ControlPortError,
  type RuntimeGraphDocument,
  type VirtualEndpointDevice,
  type VirtualEndpointRateError,
  type VirtualEndpointSettings,
} from '../../core/control-port/types';
import { waitForGraphActivation } from './waitForGraphActivation';

/** Device format writes are separate transactions, never graph-config edits.
 * The Studio owner blocks them while drafts/layout/parameters remain uncommitted. */
export function useVirtualEndpointRate({
  controlPort,
  settings,
  enabled,
  onApplied,
  onBusyChange,
}: {
  controlPort: ControlPort;
  settings?: VirtualEndpointSettings;
  enabled: boolean;
  onApplied: (document: RuntimeGraphDocument) => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const { t } = useTranslation();
  const [error, setError] = useReconciledRateError(controlPort, settings);
  const owner = useRef({ controlPort, generation: 0 });
  const release = useRef<(() => void) | null>(null);
  useLayoutEffect(() => {
    owner.current = { controlPort, generation: owner.current.generation + 1 };
    setError(null);
    return () => {
      owner.current.generation += 1;
      release.current?.();
      release.current = null;
    };
  }, [controlPort, setError]);
  const change = (endpointId: string, sampleRate: number) => {
    if (
      typeof endpointId !== 'string' ||
      !endpointId.trim() ||
      !enabled ||
      release.current ||
      !eligibleRate(
        settings?.devices.find((device) => device.spec.id === endpointId),
        sampleRate,
      )
    )
      return;
    const generation = owner.current.generation;
    const current = () =>
      owner.current.controlPort === controlPort && owner.current.generation === generation;
    release.current = () => onBusyChange(false);
    onBusyChange(true);
    setError(null);
    return applyVirtualEndpointRate(controlPort, { endpointId, sampleRate }, current)
      .then((result) => {
        if (!result || !current()) return;
        onApplied(result.document);
        if (result.persistenceError)
          setError({
            endpointId,
            sampleRate,
            kind: 'persistence',
            message: t('studio.graphSaveFailed'),
          });
      })
      .catch((cause: unknown) => {
        if (current())
          setError({
            endpointId,
            sampleRate,
            kind: 'negotiation',
            message:
              cause instanceof ControlPortError
                ? t(`audioSession.errors.${cause.body.code}`, {
                    defaultValue: t('audioSession.changeFailed'),
                  })
                : t('audioSession.changeFailed'),
          });
      })
      .finally(() => {
        if (current()) {
          release.current?.();
          release.current = null;
        }
      });
  };
  return {
    change,
    error,
    available: Boolean(
      controlPort.setVirtualEndpointSampleRate &&
        settings?.devices.some((device) => device.canSetSampleRate),
    ),
  };
}

function useReconciledRateError(controlPort: ControlPort, settings?: VirtualEndpointSettings) {
  const { t } = useTranslation();
  const [error, setError] = useState<VirtualEndpointRateError | null>(null);
  // A transport reply can be lost even though device/catalog reconciliation
  // completes. Matching native and in-memory state permits a durability check,
  // not an assumption that the catalog was saved successfully.
  const reconciled =
    error?.kind === 'negotiation' &&
    settings?.devices.some(
      (device) =>
        device.spec.id === error.endpointId &&
        device.observedSampleRate === error.sampleRate &&
        device.spec.defaultSampleRate === error.sampleRate,
    );
  useEffect(() => {
    if (!reconciled || !error || !controlPort.getGraphCatalog) return;
    let cancelled = false;
    void controlPort
      .getGraphCatalog()
      .then((catalog) => {
        if (!cancelled)
          setError((current) =>
            current !== error
              ? current
              : catalog.persistenceError
                ? { ...error, kind: 'persistence', message: t('studio.graphSaveFailed') }
                : null,
          );
      })
      .catch(() => {
        // Keep the failure visible until its outcome can actually be confirmed.
      });
    return () => {
      cancelled = true;
    };
  }, [controlPort, error, reconciled, t]);
  return [error, setError] as const;
}

function eligibleRate(settings: VirtualEndpointDevice | undefined, sampleRate: number) {
  return (
    settings?.canSetSampleRate &&
    Number.isFinite(settings.observedSampleRate) &&
    (settings.observedSampleRate ?? 0) > 0 &&
    settings.supportedSampleRates?.includes(sampleRate) &&
    sampleRate !== settings.observedSampleRate
  );
}

async function applyVirtualEndpointRate(
  controlPort: ControlPort,
  format: { endpointId: string; sampleRate: number },
  current: () => boolean,
) {
  if (!controlPort.setVirtualEndpointSampleRate) return null;
  const graph = await controlPort.getGraph();
  if (!current()) return null;
  const response = await controlPort.setVirtualEndpointSampleRate({
    expectedRevision: graph.revision,
    ...format,
  });
  if (!current() || !(await waitForGraphActivation(controlPort, response.revision, current)))
    return null;
  const document = await controlPort.getGraph();
  return current() ? { document, persistenceError: response.persistenceError } : null;
}
