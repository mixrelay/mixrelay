import { localControlPort } from '../core/control-port/localControlPort';
import type { ControlPort } from '../core/control-port/types';
import { StudioScreen } from '../screens/StudioScreen';

export function App({ controlPort = localControlPort }: { controlPort?: ControlPort }) {
  return <StudioScreen controlPort={controlPort} />;
}
