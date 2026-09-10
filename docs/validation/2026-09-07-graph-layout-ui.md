# Graph presentation persistence integration — 2026-09-07

## Delivered behavior

- Studio passes the active catalog identity into the workspace. Each graph owns a
  separately revisioned layout checkpoint; UI IDs preserve the independent source
  and sink cards of a single duplex native endpoint.
- Saved node positions and viewport are restored only after a successful layout
  read. Initial runtime/catalog requests can complete in either order without
  saving default positions over the stored layout. A catalog switch passes its new
  document immediately instead of briefly mounting the previous graph's document.
- Moving nodes, Undo/Redo and structural edits checkpoint positions. Pan/zoom
  completion checkpoints viewport. The layout route never submits a DSP graph,
  prepares application taps, changes audio revision or modifies device settings.
- Requests are serialized, coalesced after 400 ms of inactivity and bounded at ten
  seconds. Changes during a write are retained for the next write. Generation
  fencing ignores old replies after a reload, control-port replacement or disposal.
- A conflict or failed write retains the local draft and stops automatic retries.
  **Save current layout** explicitly reads the latest layout revision and submits
  the retained draft; **Load saved layout** explicitly discards presentation edits.
  Both actions leave audio routing untouched. Unsaved layout edits prevent catalog
  switching. The same localized actions exist in English, Korean and pseudo locale.
- HTTP and Tauri adapters expose GET/PUT graph layout commands on all three
  platform command registries. Storybook's in-memory control port implements the
  same graph-scoped revision contract; it is not disk-persistence evidence.

## Verification performed by the integrating owner

- UI suite: **162 passed / 29 files** at the final integrated checkpoint. Includes
  five controller tests, two hook tests, a real workspace-owner integration test,
  HTTP/Tauri route tests, Core viewport contract tests and late-owner regressions.
  TypeScript/Vite, Core boundaries, `cargo fmt --all --check` and `git diff --check`
  were rerun successfully after the final owner-lifecycle changes.
- Controller exercise: 500 position updates, one millisecond apart, produce one
  checkpoint after the final 400 ms quiet interval. A delayed first write retains
  the latest positions/viewport and advances the second request's layout revision.
  This is a request-coalescing functional assertion, not a before/after latency or
  performance improvement claim (previous layouts were not persisted at all).
- Workspace exercise restores split endpoint IDs, moves both cards and restores
  their saved positions with Cmd+Z. Layout revision advances while `applyGraph`
  remains uncalled. Initial catalog descriptors are awaited before testing actual
  processor controls; a mock button must not act before that control exists.
- macOS seven common Rust packages: **129 passed / 1 ignored**. The ignored test
  requires a separate temporary CoreAudio fixture; it was not silently counted.
- macOS desktop proxy: **4 passed**, including encoded graph path boundaries.
- TypeScript/Vite production build, selected-crate all-targets Clippy, Core UI
  boundaries, formatting and whitespace checks passed. Vite still reports the
  existing large-chunk warning; no bundle optimization is claimed.
- Ubuntu ARM64: **136 passed / 0 ignored**, build and Clippy passed.
- Windows ARM64: **143 passed / 7 ignored**, build and Clippy passed; frozen guest
  source hashes **60/60** matched. No PnP, reboot or security change in this run.

See [backend transactions](2026-09-07-graph-layout.md),
[virtual source gain](2026-09-07-virtual-source-gain.md),
[plug-in control failures](2026-09-07-plugin-control-recovery-ui.md),
[Ubuntu evidence](2026-09-07-linux-common-integration.md) and
[Windows evidence](2026-09-07-windows-parity-integration.md).

## Late-owner regressions found during integration review

- A replacement control port could return its layout before its graph document.
  The workspace previously retained the old model, consumed the new layout's
  restore marker on that old model and then replaced it with default coordinates.
  The new delayed-document test failed with default positions 48/728 instead of
  saved 33/900. Clearing the old model before the replacement read made it pass.
- Session documents are now bound to a control-port identity, not just a numeric
  revision (different engines have independent revision spaces). A screen test
  exercises engine A revision 100, engine B revision 1, B's accepted session
  revision 2 and a late A revision 101 callback. The callback must neither insert
  A's document nor evict B's newer session override. The regression failed before
  the active-owner callback guard and passes afterward.
- Global Undo/Redo is suspended while a layout reload or catalog/session exchange
  makes the workspace inert. Layout callbacks arriving after disposal cannot start
  a new autosave; StrictMode can explicitly set the same owner up again.
- Audio session settings now fence every awaited operation by control-port owner.
  Replacing or unmounting an owner releases its busy lease once; old replies cannot
  invoke the new owner's callbacks, overwrite errors or clear a newer busy lease.
  Seven settings tests cover stale replies, intermediate unmounts, duplicate
  submission, current-owner application and resetting fields only on port changes.
  The initial three stale-owner tests failed before the fix. Native IPC already in
  flight is not cancelled; timeout/recovery for a hung native operation remains a
  separate gate.

## Actual local service refresh and limits

Before restarting only the owned MixRelay service, the active catalog was backed
up and independently compared again. The new process (PID 40420, replacing 22276)
returned HTTP 200 for the new `/api/v1/graphs/graph-1/layout` endpoint. Independent
post-restart JSON comparison confirmed the exact audio document, revision 2,
catalog identity, names and both explicit channel cables were preserved. No live
layout, gain, device, default-device, HAL or sample-rate setting was changed as a
test. Existing `tauri dev` remained running with the updated desktop build.

Evidence:
`/Volumes/Linear 1/test/mixrelay/evidence/native-device-parity-20260907/layout-gain-Kx351j/`
contains `catalog-before.json`, `catalog-after.json` and `layout-after.json`.

Browser validation was attempted, but the automation tab became stuck on its
connection-error page before Storybook started. The server subsequently returned
HTTP 200, but the automation surface still rejected navigation. No successful
browser gesture, screenshot, native slider or vendor plug-in UI acceptance is
claimed. Unit tests use a React Flow mock; native gesture qualification remains
open. Abrupt process death before a debounced checkpoint acknowledges can lose
the unacknowledged layout edit; this is not an exit-flush guarantee.

Graph deletion/import/export, cross-OS device remapping and ordinary vendor-editor
opaque-state autosave remain independent catalog/product gates.
