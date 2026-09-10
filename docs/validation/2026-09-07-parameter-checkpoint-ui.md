# Direct parameter checkpoint UI — 2026-09-07

This follows the [controller durability change](2026-09-07-processor-state-controls.md).
Accepted native parameter updates now carry an optional `persistenceError`.

The UI keeps the accepted value and graph Active when storage fails, shows the
existing localized storage warning, and clears it only on a subsequent successful
checkpoint. It does not rebuild the audio graph, retry the failed storage operation,
or roll back an already accepted native value. Rejected requests and replies from
an old control port/unmounted workspace are not evidence of a successful save.

Parameter IPC is serialized across controls, while pending slider positions still
coalesce by node and parameter ID. This prevents cross-control response ordering
from replacing a newer storage warning with an older acknowledgement. Another
control's pending value is preserved. This is an ordering/correctness change; no
latency or throughput improvement is claimed. A hung IPC is still an outstanding
control-request recovery concern, not a claimed bounded native shutdown guarantee.

## Coordinator verification

- Publisher hook: 5 tests, including multi-ID coalescing/acknowledgement ordering,
  storage-warning recovery, old response rejection, control-port replacement and
  unmount.
- Workspace component: native parameter warning is visible, graph stays Active,
  a later save clears it, and `applyGraph` is never called for these direct edits.
- Full UI suite: **115 passed across 21 files**; TypeScript, production Vite build,
  Core boundary checks and focused Biome checks passed. The existing large-bundle
  warning remains (about 765 kB minified JS).
- Affected Rust suite: engine library **41**, macOS adapter **2** with **1 ignored**
  live fixture, protocol **3**, server **7** passed. All-target Clippy for those
  crates and the desktop passed. Cargo formatting was checked after fixing a
  whitespace-only prior Windows fixture change.

The workspace test mocks the canvas and control port; it is not native vendor
editor or all-OS pointer acceptance. Parameter disk checkpoint tests reconstruct
the real controller but replace native publisher/prepare with deterministic
fixtures. Opaque vendor editor state on ordinary exit, power-loss qualification,
native latency/clock synchronization and complete plug-in format recall remain
separate gates in the [matrix](../feature-parity-matrix.md).

The local development engine was rebuilt and restarted after backing up the live
graph. Its exact node/cable identities and settings survived recall, and the
catalog API plus active/submitted revision both returned successfully (revision 2).
The existing `tauri dev` process remains running. No user plug-in was inserted for
this check; it establishes installation/graph preservation, not native plug-in
checkpoint readback. Backups and the post-restart catalog are retained in
`/Volumes/Linear 1/test/mixrelay/evidence/native-device-parity-20260907/parameter-durability/`.
