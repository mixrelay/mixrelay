# Processor controls and native state snapshots

Scope: `crates/processor-host/src/lib.rs` and `rack_formats.rs`. No host audio device,
user graph, installed plug-in, service, or VM setting was changed by these checks.

## Corrected boundaries

- The previous single ID/value mailbox discarded ID 7 when ID 9 arrived before consumption.
  Slots now coalesce only the same ID; different IDs survive together. IDs are assigned once
  per worker lifetime, publishers serialize on the control side, and the consumer uses atomic
  values/dirty flags without allocation or locks. At most 4096 distinct IDs are accepted;
  capacity exhaustion is an error, not silent overwrite. This is a correctness change, not a
  performance improvement claim.
- `ProcessorControlRegistry::snapshot_state(node, timeout)` returns `{stateBase64: ...}` from
  the live native worker instance. The worker's editor/control broker flushes pending parameter
  edits and calls the native state API outside the DSP loop and engine audio callback. The
  caller must replace provider parameters with this authoritative snapshot, not replay stale
  `parameterValues` over it. Save and graph-switch integration belongs to the graph controller;
  this API must not run for every slider event.
- Snapshot transport is capped at 4 MiB, request waits at five seconds, and concurrent callers
  receive `Busy`. A timed-out request is not reused until its worker response arrives. Failures
  are typed: `Unavailable`, `Busy`, `Pending`, `Timeout`, `Unsupported`, `NativeFailure`,
  `TooLarge`, `InvalidResponse`. Failed exports do not produce an empty success value or modify
  an existing saved graph.
- The shared transport ABI is version 3. State and audio storage do not overlap; the state
  response sequence transfers read ownership until the next serialized request.
- VST3 had scan/probe branches but no `load_plugin` branch. Its actual loader is now connected.

## Focused evidence (macOS)

`cargo test -p tonenari-processor-host --lib`: **13 passed**, including:

- ID 7/9 preservation and same-ID last-wins;
- 4096-slot capacity rejection without losing accepted values;
- 64,000 concurrent parameter publications with ID/value consistency;
- binary snapshot response, over-limit response, explicit unsupported/native failures;
- timed-out request retained until acknowledgement;
- actual Apple AUPeakLimiter instance: changed parameter -> shared snapshot request -> native
  ClassInfo export on broker path -> base64 -> new native instance load -> parameter readback;
- shared audio region separation and crashed worker monitor cleanup.

The native AU test uses macOS's built-in effect, not hardware or a vendor authorization. It is
not a vendor editor gesture test and does not qualify AUv3, CLAP, VST3, or LV2 end to end.

## Explicit remaining gates

- Pinned LV2 wrapper `save_state` and `load_state` are unimplemented; exports return typed
  `Unsupported`. Full LV2 property/file state persistence remains a format implementation gap.
- AUv3 reuses AU ClassInfo state, but the pinned AUv3 loader still lacks async instantiation for
  components requiring it. No such component was qualified here.
- Pinned VST3 state currently stores only component state, not separate controller state.
  The loader fix does not establish complete VST3 native editor/preset recall.
- The 4 MiB cap bounds the IPC payload. `Plugin::save_state` returns an allocated `Vec`, so this
  layer cannot bound a vendor's internal allocation or force its call to return. Process
  isolation, existing shutdown/crash monitoring, and caller timeouts remain important; this
  is not proof of leak-free third-party code or uninterrupted audio during slow state capture.
- Windows/Linux execution of the new common tests and native per-format state round trips
  are not established by the macOS results.

## Direct parameter durability follow-up

`EngineGraphController::set_processor_parameter` now publishes the native parameter first,
updates `parameterValues` under the existing mutation lock, and checkpoints the current
catalog using atomic staged replacement. It never calls graph preparation, graph submission,
native instance replacement, or opaque native state export. Existing `stateBase64` is retained;
the explicit parameter changes overlay it on restart as intended.

The response gains optional `persistenceError`. A filesystem stage/commit failure remains an
accepted native edit with a durability warning, not an audio failure or rollback. The in-memory
catalog retains every accepted ID, so the next successful save includes previously unsaved
edits and clears the warning. Corrupt existing catalogs are preserved with a warning. Controllers
constructed without catalog storage are deliberately memory-only and return no storage warning;
normal product startup must continue to enable `with_graph_catalog`.

The parameter HTTP handler uses `spawn_blocking` because checkpoints may perform filesystem
syncs. It returns HTTP 200 with the warning for an accepted but unsaved value. Checkpoints are
synchronous on that control task; this change does not claim reduced disk writes or latency.

New focused regressions cover 48 concurrent publications across two IDs, zero graph preparations
or revision changes while editing, reconstruction of an actual controller from its saved catalog,
stage failure, rename/commit failure, recovery saving all previously accepted IDs, rejection before
native acceptance, corrupt catalog preservation, memory-only constructors, backward-compatible
optional warning serialization, and the HTTP success-plus-warning contract. Provider publication
and startup prepare use deterministic stand-ins in the catalog tests; these are controller/storage
integration tests, not a vendor plug-in process restart qualification. Native editor changes on
ordinary application exit remain outside this direct `parameterValues` checkpoint scope.
