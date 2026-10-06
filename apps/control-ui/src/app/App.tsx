import { useCallback, useState } from 'react';
import { localControlPort } from '../core/control-port/localControlPort';
import type { ControlPort } from '../core/control-port/types';
import type { LicensingPort } from '../core/licensing/LicensingPort';
import { localLicensingPort } from '../core/licensing/TauriLicensingPort';
import type { DriverPort } from '../core/driver/DriverPort';
import { localDriverPort } from '../core/driver/TauriDriverPort';
import { localUpdatesPort } from '../core/updates/TauriUpdatesPort';
import type { UpdatesPort } from '../core/updates/UpdatesPort';
import { LicenseAccess } from '../features/licensing/LicenseAccess';
import { DriverManagementAccess } from '../features/driver-status/DriverManagementAccess';
import { UpdateAccess } from '../features/updates/UpdateAccess';
import { StudioScreen } from '../screens/StudioScreen';

export function App({
  controlPort = localControlPort,
  licensingPort = localLicensingPort,
  updatesPort = localUpdatesPort,
  driverPort = localDriverPort,
  mode = new URLSearchParams(window.location.search).get('mode'),
}: {
  controlPort?: ControlPort;
  licensingPort?: LicensingPort | null;
  updatesPort?: UpdatesPort | null;
  driverPort?: DriverPort | null;
  mode?: string | null;
}) {
  if (mode === 'driver') return null;
  return (
    <GraphApp
      controlPort={controlPort}
      licensingPort={licensingPort}
      updatesPort={updatesPort}
      driverPort={driverPort}
    />
  );
}

function GraphApp({
  controlPort,
  licensingPort,
  updatesPort,
  driverPort,
}: {
  controlPort: ControlPort;
  licensingPort: LicensingPort | null;
  updatesPort: UpdatesPort | null;
  driverPort: DriverPort | null;
}) {
  const [paidStatus, setPaidStatus] = useState<{ port: LicensingPort | null; paid: boolean }>({
    port: null,
    paid: false,
  });
  const paid = paidStatus.port === licensingPort && paidStatus.paid;
  const onPaidAccessChange = useCallback(
    (value: boolean) => {
      setPaidStatus({ port: licensingPort, paid: value });
    },
    [licensingPort],
  );
  const stopEngine = controlPort.setEnginePower?.bind(controlPort);
  const onStopAudio = stopEngine
    ? async () => {
        await stopEngine(false);
      }
    : undefined;
  return (
    <UpdateAccess port={updatesPort}>
      {(action) => (
        <DriverManagementAccess port={driverPort} allowMultiDevice={paid}>
          {(driverAction, driverSession) => (
            <LicenseAccess
              additionalActions={
                <>
                  {driverAction}
                  {action}
                </>
              }
              onStopAudio={onStopAudio}
              port={licensingPort}
              onPaidAccessChange={onPaidAccessChange}
            >
              <StudioScreen
                applicationActions={
                  <>
                    {driverAction}
                    {action}
                  </>
                }
                controlPort={controlPort}
                driverSession={driverSession}
              />
            </LicenseAccess>
          )}
        </DriverManagementAccess>
      )}
    </UpdateAccess>
  );
}
