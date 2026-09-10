import type { ControlPort } from '../../core/control-port/types';

/** Only native release/activation acknowledgements finish a switch, never local button intent. */
export async function waitForEnginePower(
  port: ControlPort,
  enabled: boolean,
  current: () => boolean,
) {
  const deadline = performance.now() + 10000;
  while (current() && performance.now() < deadline) {
    const status = await port.getStatus();
    if (!current()) return null;
    if (status.enginePower?.enabled === enabled && !status.enginePower.transitioning) return status;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  if (!current()) return null;
  throw new Error('Engine power acknowledgement timed out');
}
