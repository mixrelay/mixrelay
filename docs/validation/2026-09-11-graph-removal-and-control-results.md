# Graph removal and command results

## Contract

- Catalog `delete` takes an explicit graph ID and both observed revision fences.
  It publishes the remaining catalog through the existing execution transaction,
  not a second runtime registry. The removed graph's recorders are finalized;
  shared resources still referenced by another graph remain owned.
- Deleting the selected graph selects its next neighbor, or the previous one at
  the end. Deleting the last graph creates an empty graph with a fresh identity.
  Missing/stale graph IDs do not fall back to the selected graph.
- Removal is not undoable. Existing user workspace files are not deleted.
  The sidebar offers a small per-row delete action and a confirmation naming the
  target. A settled uncertain failure can be dismissed to reach catalog reload.
- Catalog errors explicitly report `catalogOutcome: not_applied`. An accepted
  execution with a failed durable replacement remains a success with a persistence
  warning. Ambiguous transport/server failures fence subsequent mutations.
- The existing UI node writer returns `applied`, `rejected`, or `uncertain`.
  Graph identity, optional caller revision, exact acknowledgement revision, and
  post-await document identity/revision are checked before restoring the view.
  Accepted-but-unconfirmed commands are never automatically replayed.

## Executed verification

- Engine library: 246 tests passed. Added catalog removal tests cover selected,
  background, and final graph removal; stale/missing targets; persistence failure;
  recorder finalization and WAV RIFF length; same recorder ID with different
  widths across selected graphs; surviving shared-device wiring and PCM.
- Control protocol/server: 10 + 26 tests passed, including the definitive catalog
  rejection marker. Documentation generation passed for both crates.
- Focused UI: 94 tests across 8 files passed, including confirmation/cancel,
  uncertain failure dismissal, late/foreign node targets, lost replies, changed
  selection during reads, and incorrect acknowledgement revision.
- Changed UI formatting/lint, TypeScript, and UI dependency boundaries passed.
- Rust formatting and strict Clippy passed for engine, control server, and
  control protocol with all targets. Engine debug build and normal Tauri debug
  application bundle build passed. Existing Vite >500 kB chunk advisory remains.
- Independent read-only review findings were fixed and reviewed again.

## Native application check

- Before restart the existing recorder was stopped. Normal app Quit left neither
  desktop/engine process nor a listener on port 19341; the rebuilt app restarted
  and restored the existing catalog.
- Created one empty verification graph (`graph-96`, Untitled Graph 4), checked
  confirmation text and initial Cancel focus, canceled once, then deleted that
  verification graph. The three original graph IDs remained, with `graph-53`
  selected. No user graph, saved workspace, or recording file was removed.
- Actual recording finalization and shared PCM were exercised by deterministic
  controller/recorder tests, not by recording from the user's live devices.

## Follow-up external-control work

The subsequent local-control implementation is documented in
`2026-09-11-local-controls-and-stream-deck.md`. It adds private local IPC,
controls-only Undo/Redo reconciliation, gain commands and a Stream Deck adapter
using the same desktop capture-aware execution boundary.
The full device-dependent native fixture suite was not run for this change.
This is a functionality/safety change, not a measured performance optimization.
