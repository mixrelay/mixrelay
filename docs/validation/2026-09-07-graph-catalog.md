# Durable graph catalog — 2026-09-07

## Implemented boundary

The previous Studio sidebar stored only `{ id, name }` in React. Selecting or creating
an entry changed the heading, not the engine document; a reload lost the list.
The sidebar now uses the engine-owned graph catalog and confirms the target audio
revision before replacing its workspace.

- `GET /api/v1/graphs`: identities, names, active identity, authoritative active document,
  catalog revision and an optional persistence warning.
- `GET /api/v1/graphs/{id}`: document preflight without selection changes; the desktop
  proxy can prepare application capture before requesting a switch.
- `PUT /api/v1/graphs`: create an independent empty document, select a saved document,
  or rename. Requires both the expected graph revision and catalog revision.
- One engine mutation lock serializes catalog actions, graph applications, parameter
  updates and session changes. Graph/metadata revisions are distinct; renaming does
  not rebuild DSP or reset the active workspace history.
- `graphs.json` is versioned, per-user, atomically staged beside the destination,
  synchronized and replaced using the existing platform storage primitive. Limits:
  256 graphs, 256-byte names, 16 MiB serialized catalog; graph request bodies also
  have a 16 MiB limit. Saved nodes, edges, channel maps and gains are independent.
- On first use, the catalog imports the already prepared controller document.
  It does not replace an existing saved file with defaults. Full-catalog recall owns
  startup attachment restoration; endpoint preferences still retain recreation settings.
- On startup, native resource preparation is required before restoring the saved active
  graph. Invalid/unsupported/corrupt catalogs stay untouched; the fallback engine remains
  available and editable, while catalog mutations are unavailable.
- Endpoint attachments follow the current engine-wide negotiated sample rate when a
  saved graph is selected; selecting a graph does not change the global audio clock.

## Failure behavior

Validation, revision conflicts and disk **staging** failures reject before native
publication. Native resource failure cancels this transaction's prepared endpoint;
the old graph, catalog identity and saved file are retained.

After native preparation succeeds, a disk **replacement** failure is deliberately not
reported as audio rollback. The processor is published, in-memory selection matches it,
and `persistenceError` is returned on successful graph/catalog responses. The UI keeps
audio Active and separately warns that restarting can lose unsaved changes. A later
successful write clears the warning. No-op selection does not hide an existing warning.
This avoids orphaning native pending resources when the filesystem fails after preparation.

The UI disables catalog mutations while the graph application queue is loading,
applying or blocked, while native parameter writes are pending, or during session changes.
A lost mutation acknowledgement leaves the old draft visible but inert until an explicit
authoritative reload. Requests are bounded, stale replies are generation-fenced, and
metadata-only renames do not replace the workspace document. Rename typing is debounced.

## Plug-in state boundary

Create/select captures each active hosted processor's state on its control worker before
destroying that graph. The whole capture has a five-second deadline. A failed, missing or
unsupported snapshot rejects the switch and preserves the current graph/file; state is
not silently discarded. Native state replaces stale parameter overlays.

This is not a claim that every plug-in format/session recall is qualified. See
[processor state validation](2026-09-07-processor-state-controls.md). LV2 state is explicitly
unsupported; VST3 controller-specific state remains a separate gate.

## Focused validation

Local native Rust catalog fixtures use exclusively owned temporary configuration
directories; they do not modify the installed service or user graphs.

Nine engine tests cover:

1. Independent create/select, gain edits, rename and process-controller restart recall.
2. Stale graph writers and pending audio publication.
3. Corrupt file preservation with an editable fallback engine.
4. Native target rejection preserving document, active identity and saved bytes.
5. Pre-native disk staging failure.
6. Post-native disk rename failure: audio activation, warning, no-op warning retention,
   unchanged previous saved bytes, then successful subsequent save/recovery.
7. Concurrent metadata revision conflict without audio revision changes.
8. Prepared endpoint cancellation after resource failure, without cancelling a previous
   successful pending processor.
9. Duplicate identities/unsupported catalog versions never being reinitialized.

The HTTP route contract test covers both read routes, successful rename, stale metadata
revision and malformed mutation rejection. UI focused tests cover HTTP/Tauri forwarding,
independent create/select, metadata-only history retention, native rejection, uncertain
acknowledgement recovery, stalled request bounds and disposal/late-activation fencing.

Commands used:

```sh
cargo test -p tonenari-engine-service --lib graph_catalog_tests
cargo test -p tonenari-control-server --lib catalog_routes
cargo clippy -p tonenari-engine-service -p tonenari-control-server -p tonenari-control-protocol --lib --tests -- -D warnings
pnpm --filter @tonenari/control-ui exec vitest run src/features/graphs/GraphCatalogController.test.ts src/app/App.test.tsx src/core/control-port/HttpControlPort.test.ts src/core/control-port/TauriControlPort.test.ts
pnpm --filter @tonenari/control-ui typecheck
```

Engine catalog **9 passed**; HTTP route **1 passed**; focused UI **17 passed**; common Rust Clippy and UI TypeScript/
Biome passed before handoff. Native Windows/Linux reruns and installed macOS application
validation are coordinated separately by the parent task; do not infer their results from
these local fixture tests.

Catalog-specific storage errors are distinguished from engine connection failures in both
the sidebar and create dialog; the focused UI test asserts this distinction.

## Still open

- Direct hosted parameter edits now have an independent durable checkpoint; see
  [parameter state controls](2026-09-07-processor-state-controls.md). Opaque native
  editor changes still require snapshotting during a switch; general shutdown/crash
  recall of those editor changes remains unimplemented.
- Exact installed-app create/select/restart behavior with real AU/VST3/CLAP instances,
  native capture, permission dialogs and editors needs OS-specific end-to-end evidence.
- The macOS desktop capture commit is a separate boundary after an engine-accepted switch;
  a failure there must not be represented as proof of successful native capture.
- Migration imports the controller's startup document. A pre-upgrade live graph that was
  never persisted must be backed up before restarting the old engine and reapplied; there
  is no invented reconstruction of previously unsaved cables or plug-in state.
- Graph deletion/export/import UI and cross-OS device identity remapping are not introduced
  by this create/select/rename slice. Saving a platform-specific device or unavailable
  plug-in does not make it portable to another OS.
- Ctrl+N opens the create flow, but the existing tooltip still displays the macOS glyph;
  platform shortcut presentation remains tracked separately.

No performance improvement is claimed by this change. Disk serialization and native
plug-in snapshots stay off the audio thread; storage work and snapshot latency remain
subjects for future measurement rather than an unmeasured performance claim.
