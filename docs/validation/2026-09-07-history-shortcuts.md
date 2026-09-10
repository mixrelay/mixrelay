# Control history and platform shortcut validation — 2026-09-07

## Scope and result

Completed shared UI boundary changes for `history_coalescing` and the new-graph tooltip portion
of `platform_shortcuts`. This is not native macOS/Windows/Linux pointer or keyboard E2E evidence.
No native audio, plugin processing, or device-default configuration was changed.

## Observed defect and correction

The workspace originally grouped every parameter change on a node by `${kind}:${nodeId}`.
An actual regression using the compressor model and its threshold/ratio update functions failed
before the fix: both keys were `level:compressor-1`. Consequently, adjusting two distinct controls
within 400 ms could merge their undo histories.

`features/routing-workspace/graphHistoryGroup.ts` now includes the changed gain/parameter identity.
A single control preserves its first value and latest value; switching controls creates a new entry.
Changes to multiple controls, multiple nodes, node identity, or cable collections do not use the
single-control debounce grouping. The same threshold/ratio regression passes after the fix.

The core history hook also flushes a pending control entry before a normal structural `record`,
and will not merge different operation kinds merely because a caller reused a group string.
The existing default 400 ms inactivity debounce remains unchanged.

| Case | Verified result |
| --- | --- |
| 500 successive values on one control, 1 ms between values | One undo restores the first value; one redo restores value 500 |
| 399 ms idle / 400 ms idle | One pending timer / zero pending timers |
| Threshold → ratio → threshold within 400 ms | Three separately reversible gestures |
| Undo before the debounce deadline | Pending gesture is flushed and undone immediately; timer removed |
| New gesture after undo | Stale redo is invalidated |
| Structural edit after an unfinished control gesture | Structural edit undoes first, control gesture second |
| Owning control port replaced | Pending and committed history cleared; no undo replay into the new port |
| Workspace/hook unmounted | No scheduled history timer remains |

The numbers above are functional assertions, not a CPU, memory, or latency improvement claim.
No performance benchmark is claimed by this change.

## Platform shortcut display

`StudioScreen.tsx` and the shortcut story now pass the semantic `Mod+N` token rather than the
macOS-only literal `⌘N`. `core/ui/platformShortcut.ts` formats that token inside `ShortcutTooltip`:

| Reported platform | Tooltip |
| --- | --- |
| MacIntel | ⌘N |
| Win32 | Ctrl+N |
| Linux x86_64 | Ctrl+N |
| Linux aarch64 | Ctrl+N |

The component tests mount the actual Mantine tooltip, dispatch hover, and advance its 350 ms open
delay plus the portal transition frame. No inline shortcut badge was introduced. Explicit shortcuts
without a `Mod+` token are preserved. Existing keyboard handlers were not broadened or replaced.

## Reproducible verification

Run from the repository root:

```sh
pnpm --filter @mixrelay/control-ui exec vitest run \
  src/features/routing-workspace/RoutingWorkspace.test.tsx \
  src/core/ui/ShortcutTooltip.test.tsx \
  src/core/history/useUndoHistory.test.ts \
  src/features/routing-workspace/graphHistoryGroup.test.ts
pnpm --filter @mixrelay/control-ui typecheck
pnpm --filter @mixrelay/control-ui check:boundaries
```

The focused run at 19:25 KST passed **4 files / 26 tests**: 12 workspace tests, 8 core history
tests, 5 tooltip tests, and 1 graph-history-group regression. TypeScript, the Core UI boundary
check, and Biome on the 9 changed owned source/test files passed. The control-port replacement
test renders the real workspace with a mocked Canvas input action and fake native control port;
it does not claim to exercise a real OS slider or driver.

## Matrix handoff

- `history_coalescing`: implemented; focused component/hook evidence strengthened by this document.
  Its existing component-level history acceptance is now satisfied. The coordinator
  closes this shared-contract issue; native slider/keyboard E2E on all OSes stays
  open under the separate gesture, source-gain and platform-shortcut gates.
- `platform_shortcuts`: new-graph modifier display is implemented and component-tested for the
  listed platform identities. Do not mark the broader native UI/shortcut equivalence row complete:
  real OS window-manager conflicts, text-input handling, and native app keyboard E2E remain open.
- Any older matrix note saying the new-graph tooltip is hard-coded to Cmd is now stale and should
  reference this validation instead.
