# UI menu harness and failed-session priority — 2026-09-07

## Menu investigation

The integrating owner observed one failure in the full UI run: 264 passed, one failed.
`AudioGraphCanvas.test.tsx` had received its real `onNodeMenuOpenedChange(true)` notification,
but `findByRole('menuitem', { name: 'Test source' })` could not see the dropdown. The captured
element had `opacity: 1` and `display: none`; the timeout was not an unreceived open event.

An independent unchanged-source rerun passed both the focused 14 Canvas tests and all 265
tests. The original failure is therefore recorded as intermittent, not claimed independently
reproduced in this investigation.

Installed Mantine 9.6.0 source explains the contradictory state: Popover computes
`referenceHidden` from floating-ui's `hideDetached` middleware unless its provider environment
is `test`; PopoverDropdown maps that result to `display: none`. jsdom supplies zero-size
geometry rather than browser layout. The async geometry result can race a visible-role query
even though the controlled menu state is open. This matches the reported inline style more
specifically than a slow enter animation would.

Only `AudioGraphCanvas.test.tsx` now uses a test-local MantineProvider with `env="test"` and the
actual ToneNari theme. Mantine's installed test mode disables the nonphysical detached-reference
decision and animation rendering. Real menu components, event callbacks, selections, focus
behavior and pointer dismissal remain in the test. No production menu code or timeout was changed.
These tests still do not prove browser geometry, clipping or physical pointer behavior.

## Actual state-priority correction

`virtualNodeStatus.ts` previously checked non-active graph state before known processing failure.
A stopped/failed owner could therefore leave an indefinitely pending graph with an amber node
instead of a red failure marker. Known `processingState: stopped` or `issue: processing_failed`
now takes priority over loading/applying/blocked; engine-unavailable remains red. A pending
healthy session remains amber, and meter amplitude is not used as health evidence.

Six regression cases (failed while applying and stopped while loading, each in English, Korean
and pseudo locale) were added to `virtualNodeStatus.test.tsx`. The existing pending-healthy,
missing session, old engine, no-signal, aggregate-warning and recovery cases remain.

## Final validation

Commands ran in `apps/control-ui` via `pnpm --filter @tonenari/control-ui exec`:

- Focused Canvas/status tests: **66 passed across two files** (14 Canvas + 52 status).
- `vitest run --coverage.enabled=false`: **271 passed across 38 files**, twice consecutively
  under the normal parallel suite configuration (22:20:47 and 22:20:54 KST).
- TypeScript, scoped Biome and the Core boundary check passed.

No native audio, OS-device, vendor-editor or browser geometry acceptance is implied by these
unit/component results. This is deterministic harness repair plus a correctness fix, not a
performance optimization or a measured speedup claim.
