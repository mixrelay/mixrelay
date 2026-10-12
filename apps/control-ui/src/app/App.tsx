import { useCallback, useState } from 'react';
import { localControlPort } from '../core/control-port/localControlPort';
import type { ControlPort } from '../core/control-port/types';
import type { DriverPort } from '../core/driver/DriverPort';
import { localDriverPort } from '../core/driver/TauriDriverPort';
import { FeatureAccess } from '../core/licensing/FeatureAccess';
import type { LicensingPort } from '../core/licensing/LicensingPort';
import { localLicensingPort } from '../core/licensing/TauriLicensingPort';
import type { RuntimePort } from '../core/runtime/RuntimePort';
import { localRuntimePort } from '../core/runtime/TauriRuntimePort';
import { localUpdatesPort } from '../core/updates/TauriUpdatesPort';
import type { UpdatesPort } from '../core/updates/UpdatesPort';
import { DriverSessionAccess } from '../features/driver-status/DriverSessionAccess';
import { LicenseAccess } from '../features/licensing/LicenseAccess';
import { RuntimeConnection } from '../features/runtime-connection/RuntimeConnection';
import { UpdateAccess } from '../features/updates/UpdateAccess';
import { StudioScreen } from '../screens/StudioScreen';

export function App({
  controlPort = localControlPort,
  licensingPort = localLicensingPort,
  updatesPort = localUpdatesPort,
  driverPort = localDriverPort,
  runtimePort = localRuntimePort,
  mode = new URLSearchParams(window.location.search).get('mode'),
}: {
  controlPort?: ControlPort;
  licensingPort?: LicensingPort | null;
  updatesPort?: UpdatesPort | null;
  driverPort?: DriverPort | null;
  runtimePort?: RuntimePort | null;
  mode?: string | null;
}) {
  if (mode === 'driver') return null;
  return (
    <GraphApp
      controlPort={controlPort}
      licensingPort={licensingPort}
      updatesPort={updatesPort}
      driverPort={driverPort}
      runtimePort={runtimePort}
    />
  );
}

function GraphApp({
  controlPort,
  licensingPort,
  updatesPort,
  driverPort,
  runtimePort,
}: {
  controlPort: ControlPort;
  licensingPort: LicensingPort | null;
  updatesPort: UpdatesPort | null;
  driverPort: DriverPort | null;
  runtimePort: RuntimePort | null;
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
  const workspace = (openDriverInstaller: (() => void) | undefined, action: React.ReactNode) => (
    <DriverSessionAccess port={driverPort} allowMultiDevice={paid}>
      {(driverSession) => (
        <LicenseAccess
          presentation="footer"
          port={licensingPort}
          onPaidAccessChange={onPaidAccessChange}
        >
          {(applicationInfo) => (
            <FeatureAccess value={licensingPort === null || paid}>
              <RuntimeGate port={runtimePort} onInstall={openDriverInstaller}>
                <StudioScreen
                  applicationInfo={applicationInfo}
                  applicationActions={action}
                  controlPort={controlPort}
                  driverSession={driverSession}
                  onInstallDriver={openDriverInstaller}
                />
              </RuntimeGate>
            </FeatureAccess>
          )}
        </LicenseAccess>
      )}
    </DriverSessionAccess>
  );
  return (
    <UpdateAccess port={updatesPort}>
      {(action, openDriverInstaller) => workspace(openDriverInstaller, action)}
    </UpdateAccess>
  );
}

function RuntimeGate({
  port,
  onInstall,
  children,
}: {
  port: RuntimePort | null;
  onInstall?: () => void;
  children: React.ReactNode;
}) {
  return port ? (
    <RuntimeConnection port={port} onInstall={onInstall}>
      {children}
    </RuntimeConnection>
  ) : (
    children
  );
}
