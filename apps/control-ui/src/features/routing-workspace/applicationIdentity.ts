import type { CapturableApplication } from '../../core/control-port/types';

/** Presentation identity follows the selected source, never a display name or a current PID guess. */
export function applicationSelectorKey(selector: unknown): string | undefined {
  if (!selector || typeof selector !== 'object') return undefined;
  const value = selector as Record<string, unknown>;
  if (value.match === 'bundle_id' && typeof value.bundle_id === 'string' && value.bundle_id) {
    return JSON.stringify(['bundle', value.bundle_id]);
  }
  if (
    value.match === 'process_id' &&
    Number.isInteger(value.process_id) &&
    Number(value.process_id) > 0
  ) {
    return JSON.stringify(['process', value.process_id]);
  }
  if (value.match === 'executable_path' && typeof value.path === 'string' && value.path) {
    return JSON.stringify(['path', value.path]);
  }
  // All-except is a set of applications, not one app whose icon can be inferred.
  return undefined;
}

export function applicationInventoryKey(application: CapturableApplication): string {
  return (
    applicationSelectorKey(application.selector) ?? JSON.stringify(['bundle', application.bundleId])
  );
}

/** A menu choice for one PID must not resolve to another instance sharing its application ID. */
export function applicationItemId(application: CapturableApplication): string {
  return `application:${applicationInventoryKey(application)}`;
}
