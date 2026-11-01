import { useCallback, useState } from 'react';
import { FeatureAccess } from '../core/licensing/FeatureAccess';
import type { ControlPort } from '../core/ports/control/ControlPort';
import type { DiagnosticsPort } from '../core/ports/DiagnosticsPort';
import type { DriverPort } from '../core/ports/DriverPort';
import type { LicensingPort } from '../core/ports/LicensingPort';
import type { MidiPort } from '../core/ports/MidiPort';
import type { RuntimePort } from '../core/ports/RuntimePort';
import type { UpdatesPort } from '../core/ports/UpdatesPort';
import { DriverSessionAccess } from '../features/driver-status/DriverSessionAccess';
import { LicenseAccess } from '../features/licensing/LicenseAccess';
import { MidiAccess } from '../features/midi/MidiAccess';
import { RuntimeConnection } from '../features/runtime-connection/RuntimeConnection';
import { UpdateAccess } from '../features/updates/UpdateAccess';
import { StudioScreen } from '../screens/StudioScreen';
import { localPorts } from './localPorts';

/** Omitted ports are this environment's own (app/localPorts); tests and stories pass fakes. */
export function App({
  controlPort = localPorts().controlPort,
  licensingPort = localPorts().licensingPort,
  updatesPort = localPorts().updatesPort,
  driverPort = localPorts().driverPort,
  runtimePort = localPorts().runtimePort,
  diagnosticsPort = localPorts().diagnosticsPort,
  midiPort = localPorts().midiPort,
}: {
  midiPort?: MidiPort | null;
  controlPort?: ControlPort;
  licensingPort?: LicensingPort | null;
  updatesPort?: UpdatesPort | null;
  driverPort?: DriverPort | null;
  runtimePort?: RuntimePort | null;
  diagnosticsPort?: DiagnosticsPort | null;
}) {
  return (
    <MidiAccess port={midiPort}>
      <GraphApp
        controlPort={controlPort}
        licensingPort={licensingPort}
        updatesPort={updatesPort}
        driverPort={driverPort}
        runtimePort={runtimePort}
        diagnosticsPort={diagnosticsPort}
      />
    </MidiAccess>
  );
}

function GraphApp({
  controlPort,
  licensingPort,
  updatesPort,
  driverPort,
  runtimePort,
  diagnosticsPort,
}: {
  controlPort: ControlPort;
  licensingPort: LicensingPort | null;
  updatesPort: UpdatesPort | null;
  driverPort: DriverPort | null;
  runtimePort: RuntimePort | null;
  diagnosticsPort: DiagnosticsPort | null;
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
                  diagnosticsPort={diagnosticsPort}
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
