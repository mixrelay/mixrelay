import type { ControlPort } from '../../core/control-port/types';

/** Wait for native adoption after a format write. Cancellation stops further reads;
 * deadline or control failures propagate so the caller retains its existing document. */
export async function waitForGraphActivation(
  controlPort: ControlPort,
  revision: number,
  isCurrent: () => boolean,
) {
  const deadline = performance.now() + 3000;
  while (performance.now() < deadline) {
    if (!isCurrent()) return false;
    const status = await controlPort.getGraphStatus();
    if (!isCurrent()) return false;
    if (status.activeRevision >= revision) return true;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  if (!isCurrent()) return false;
  throw new Error('Graph activation timed out');
}
