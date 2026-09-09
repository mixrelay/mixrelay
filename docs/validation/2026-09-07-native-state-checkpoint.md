# Native editor state checkpoint boundary

## Implemented control-plane transaction

`EngineGraphController::checkpoint_processor_state(Some(node_id))` captures one
provider; `None` captures all providers in the active graph. The public entry owns
the existing controller mutation lock. The crate-visible `*_locked` and `*_with`
entries require their caller to own that same lock; the injectable capture function
also supports matching a native editor-close observation to its exact worker.

The sequence is active-publication guard, bounded native capture (five seconds
aggregate), complete catalog staging, atomic replacement, then in-memory graph and
catalog replacement. No DSP preparation, native resource replacement, or audio /
catalog revision increment occurs. Native `stateBase64` replaces the old provider
parameter object so stale `parameterValues` cannot overlay the captured preset.

Any capture or storage failure leaves every saved provider and the original disk
file unchanged. A persistence warning is recorded, and the lifecycle caller receives
an error instead of permission to claim a successful save. The existing live worker
is not destroyed by this helper, so an explicit close can be refused and retried.

## Deterministic tests

`apps/engine-service/src/graph_native_state_tests.rs` adds four tests:

- Dirty vendor state replaces stale parameter overlays; actual catalog file survives
  controller destruction/restart; zero audio preparations during checkpoint.
- A second provider's unsupported, timeout, native-failure, or unavailable snapshot
  rolls back the whole document, even after the first provider captured successfully.
- Staging and final replacement failures preserve the document/file; a same-revision
  retry succeeds and clears the persistence warning.
- Unknown provider IDs and pending graph publication are rejected before capture.

These tests inject the native capture result while exercising real storage and
controller restore. They do not qualify a particular vendor plug-in's native state
API, native window close detection, OS shutdown wiring, or abrupt process/power loss.
Those lifecycle integrations and platform checks require their own evidence.

## Preserving live state through structural edits

The ordinary graph PUT accepts the legacy document body and an additive optional
`processorParameterChanges` array. Each entry names `nodeId`, `typeId`, numeric
`parameterId`, and normalized `value`. A maximum of 4096 unique node/parameter
entries is accepted; unknown, replaced, read-only, duplicate, and invalid-value
targets are rejected before requesting native state.

For a surviving provider with the same node identity and type, the live preset is
captured before its replacement instance is prepared. The client's entire stale
`stateBase64`/`parameterValues` object is not replayed. Only explicit control deltas
are overlaid on the captured state; this preserves intentional parameter Undo/Redo
without reverting unrelated native-editor edits. New nodes and changed provider
types retain their explicitly supplied initial configuration. Whole-preset replacement
is not guessed from document differences and requires a future explicit restore API.

The HTTP boundary moves the potentially blocking capture/preparation to a blocking
control task. Pending graph publication rejects capture because the current document
and native worker must refer to the same active publication. A failed capture leaves
both the proposed document and current graph unchanged. Startup/catalog switching
deliberately use the restore boundary rather than this live-edit preservation boundary.
Reselecting the already active catalog entry explicitly checkpoints native state and
returns its updated document without changing the audio or catalog revision.

`graph_live_state_tests.rs` covers legacy stale-object preservation, explicit parameter
Undo/Redo overlays, new/replaced identities, all-or-nothing multi-provider failure,
invalid/read-only delta rejection, pending/stale publication, and failed same-graph
reselection. Protocol and server tests cover the additive legacy-compatible wire shape,
full intent forwarding, malformed-entry rejection, and fail-closed older backends.

Local common-boundary verification after this slice: engine-service library 66/66,
control-protocol 6/6, control-server 10/10 tests passed; Clippy for those three
packages (`--lib --tests -- -D warnings`) passed. These totals include concurrent
editor lifecycle and restored-process-identity tests; they are not OS VM or vendor
UI qualification counts.
