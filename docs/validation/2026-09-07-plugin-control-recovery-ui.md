# Plug-in scan and editor control recovery UI

The packaged route is `RoutingWorkspace` -> `TauriControlPort` -> native
`control_proxy.rs` -> engine HTTP control. The proxy and client preserve failure responses;
the former workspace hooks discarded rejected promises. This change removes those silent
catches without changing native plug-in loading, discovery protocols, or editor hosting.

## Implemented

- Scan/catalog, directory-picker and search-path storage failures have distinct localized
  alerts and explicit retry. A failed scan preserves its last valid catalog; it does not turn
  a failure into a successful empty list. Retrying a scan after selecting a directory reuses
  that selection rather than reopening the picker. Picker cancellation is not a success or
  failure and does not clear an unresolved prior error.
- A catalog session serializes its native requests. Port replacement/unmount invalidates
  late responses and prevents a late directory dialog from writing obsolete search paths.
- Editor read, open/close rejection, and native failed/unavailable states produce an alert
  naming the affected node. Retry repeats the failed operation, including `close(false)`.
  Transport failure retains the last confirmed state instead of pretending a window opened
  or closed. Failed polling pauses until explicit retry.
- Per-node command sequence guards prevent an old status read from replacing a newer command
  response. Port and node identity lifetimes prevent removed/replaced nodes from being revived
  by late callbacks. Timers are cleared on disposal; duplicate pending commands are ignored.
- English, Korean and pseudo translations are included. Existing Core `RetryAlert` supplies
  accessible alert semantics and a disabled retry while a recovery request is pending.

## Evidence

Focused Vitest run: **35 passed across five files**:

- `useProcessorCatalog.test.ts`: 5 lifecycle/storage/retry cases.
- `useProcessorEditors.test.ts`: 7 request ordering, stale callback and retry cases.
- `ProcessorControlAlerts.test.tsx`: 4 rendered alert cases, including all three locales
  through the actual `TauriControlPort` with mocked IPC responses, and failed-close retry.
- Existing `RoutingWorkspace.test.tsx` and `TauriControlPort.test.ts` regressions.

TypeScript, scoped Biome checks, and Core boundary validation pass. Native invoke and directory
dialogs remain uncancellable: these guards ignore obsolete results rather than claiming to
cancel an operating-system operation. No installed application or vendor AU/VST3/CLAP/LV2
editor was exercised here. Individual scanner-child failures that the backend itself converts
into an empty catalog are a separate native discovery diagnostics gap; UI cannot infer an
error from a successful response. Per-format native editor qualification remains open.
