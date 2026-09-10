# Isolated inline plug-in UI — macOS implementation and validation

## Required result

The actual plug-in editor belongs inside its graph node. An AU/VST instance must
remain in its own worker process; an editor or DSP crash/hang must not block the
engine or unrelated nodes. A floating window or a parameter-only reconstruction is
not the requested result.

## Implemented boundary correction

`NativeEditorHost` now accumulates pending native lifecycle events independently
of the plug-in mutex. `tick_editor` pumps native events first, then attempts the
mutex and consumes pending events only on success. Close is sticky, supersedes
resize, and is cleared when its native window is retired. Multiple resizes coalesce
to the latest size. Explicit plug-in methods remain serialized; no borrowed editor
is sent to another thread and no raw-pointer ownership split was introduced.

This removes event-loop-sized mutex holds. It does not prove that arbitrary vendor
editor methods are nonblocking or that all third-party native code is thread-safe.

## Native feasibility evidence

Command: `cargo run -p tonenari-processor-host --example editor_surface_probe`.
The Rust parent gives its own disposable child 20 seconds, then kills and reaps
that exact child on timeout. No user graph, audio endpoint, or installed engine is
changed. The probe creates a hidden AppKit window and an actual Apple AUBandpass
view; it never orders a window onscreen or captures another application's content.

- Actual Cocoa drawing produced a 1000 × 400 PNG for a 500 × 200 logical view.
- Queued native mouse events changed the actual AU frequency from 5000 Hz to
  456.4656982421875 Hz, and the rendered image changed with it.
- Initial AppKit event processing must complete before synthesized coordinates are
  created. Otherwise the first event dequeue transformed their Y coordinate and
  the control received events outside its bounds.
- Ten close/reopen cycles preserved the actual parameter value. The parent had
  zero subviews after each close and exactly one after each reopen. This checks
  attachment ownership, not a complete memory-leak audit.

### Matched mutex-exclusion measurement

Same disposable AUBandpass instance, debug build, 48 kHz, 32-sample stereo blocks,
60 native redraws, identical DSP loop with a 100-microsecond sleep. Measurements
ran sequentially without another build/test/benchmark running alongside them.

| Native redraw boundary | Elapsed | Successful DSP blocks during interval |
| --- | ---: | ---: |
| Plug-in mutex retained across redraws | 248.896 ms | 0 |
| Native redraws without plug-in mutex | 250.465 ms | 1920 |

Elapsed time includes one PNG encode/write after the 60 redraws. This experiment
demonstrates exclusion/progress, not realtime throughput, actual callback deadline
compliance, an end-to-end latency measurement, or production `tick_editor` coverage.
The DSP probe rejects process errors and non-finite PCM. It does not claim that an
arbitrary sustained native tracking loop has passed a live-audio test.

Apple's own loaded frameworks emitted duplicate-class warnings for
`RendezvousService` and `NetServiceDirectory`; they were not suppressed or modified.
The native probe exited successfully despite those system warnings.

## Node integration

The macOS worker now owns a hidden native parent, bounded frame mailbox and ordered
pointer ring. A main-run-loop timer also runs during AppKit mouse tracking. The
desktop displays the actual PNG surface inside the graph node and maps pointer
coordinates through the displayed rectangle (including graph zoom). One frame
request and one ordered input request are in flight per node. Frame payload,
physical capture area and input queue sizes have explicit limits.

Normal pointer release does not publish cancellation after a completed click.
Closing, disposal, queue saturation and native checkpoints publish out-of-band
gesture cancellation. Native-state snapshots wait for frame ownership rather than
overwriting a pending response. Native resize events update content size, frame
geometry and input coordinates together. Desktop node resizing scales the captured
editor while preserving its aspect ratio; it does not force the vendor view to
resize. Input mapping excludes letterboxes and clamps an active drag to the view.
This path adds no C source or unsafe Send/Sync.

The editor opens automatically inside the node, without a collapse control.
Manufacturer text, the shared level meter and the editor/state/action footer were
removed. Failed editors retain a retry action and diagnostics; unavailable editors
retain parameter fallback. Connection ports start below the header.

All nodes have a bottom-right resize grip. Explicit sizes persist in the existing
graph layout store; layout Undo preserves the audio revision and does not submit
DSP changes. Natural measured size is distinct from an explicitly saved size, and
visible controls determine the minimum height.

## Routing-only changes retain the real instance

The previous factory created a worker for every provider whenever a graph plan was
prepared. A surviving provider now returns an ownership-transfer request keyed by
the actual instance token, only when runtime node ID, type, current native state,
format and channels match. The exchange validates all owners before native
activation, then moves the complete Box (including PCM cursor) at the boundary.
No audio-thread allocation, mutex or child-process destruction is added.

Retired/prepared owners cannot be reused. Preparation is rejected before calling
the factory while a plan is pending, and production callers retain the mutation
guard until submission. These checks cover deletion/Undo and pending-activation
races found by independent review. Configuration failure leaves the active owner
untouched; retired placeholders are reclaimed off the audio thread.

## Verification and remaining work

- Processor-host unit tests: 41 passed, 0 failed, 0 ignored. The independent
  processor test exercises 64 actual mailbox/PCM cycles while the other mailbox
  stalls and transitions to failed; it is not a live vendor-crash integration test.
- Native probe: successful exit, real rendering/input, concurrent DSP, and ten
  attachment cycles as above. `cargo test --all-targets` compiles the example but
  does not run its main; the separate `cargo run` result is essential evidence.
- Strict processor-host/engine-service Clippy (all targets), workspace formatting,
  engine-service build, and processor-host documentation build passed.
- Audio-engine unit tests: 36 passed, 0 failed, 0 ignored, including ownership
  transfer, cursor preservation, configuration failure, missing owner and retirement.
- Engine-service focused native-editor tests: 4 passed; pending-plan preparation
  gate test: 1 passed. An initial unmatched test filter ran zero tests; it was not
  counted as validation and was corrected to the matching native-editor filter.
- Read-only review found and verified fixes for click cancellation, checkpoint
  deadlock, duplicate Open, non-finite/resize bounds, retired reuse and pending-plan
  races. No new source blocker remained at the final review.

**Remaining limitations:** keyboard/wheel/right-click input, popup/file-dialog
embedding, universal GPU-editor capture and Windows/Linux inline surfaces are not
implemented or verified here. Actual Apple AUBandpass rendering inside the running
desktop node was observed after local engine/app replacement. This does not establish
compatibility with every AU/VST or constitute a complete memory-leak audit.

Public API constraints to retain when extending the transport:

- [NSView cacheDisplay](https://developer.apple.com/documentation/appkit/nsview/cachedisplay%28in%3Ato%3A%29)
  supports the verified Cocoa view; it is not a universal GPU capture contract.
- [AppKit mouse tracking](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/EventOverview/HandlingMouseEvents/HandlingMouseEvents.html)
  can run synchronously until mouse-up. Subsequent input must not depend on the
  blocked event dispatch returning, and DSP must not hold/wait for that dispatch.
- [VST3 IPlugView](https://github.com/steinbergmedia/vst3_pluginterfaces/blob/master/gui/iplugview.h)
  uses a native parent view. A pointer in the worker cannot simply be installed
  into the desktop process's web view.

Do not describe this checkpoint as verified crash containment for every vendor.
Exercising an intentionally crashed/hung vendor instance alongside an independent
audible route remains required acceptance work.

## Local checkpoint — 2026-09-10

The local engine and desktop were rebuilt and reopened. The actual AUBandpass
editor was visible inside its node. The resize grip worked, and saved node sizes
survived an app/engine restart. A zero-height surface regression was corrected and
actual rendering was checked again. The final observed engine status was
ready/running at 64 samples, with zero underruns/overruns and no reported issue.
This is a point-in-time observation, not a sustained audio-quality test.

Native initialization timeout during catalog restore no longer makes a valid
saved catalog unreadable. Desired documents remain available while the existing
maintenance loop retries activation. Providers that are not running retain saved
state instead of replacing it with a missing worker snapshot. Corrupt storage
remains protected. The original device-initialization timeout's root cause is not
claimed to be eliminated: injected retry tests and a healthy final live restart
are different evidence.

The UI retries initial, idempotent catalog reads with a 500 ms backoff capped at
5 seconds. Disposal cancels retry. Mutations, protected-storage failures and an
already loaded workspace are not automatically replayed or replaced.

Implementation verification ran in separate, overlapping test selections; these
counts must not be added together:

- UI suite: 193 passed; initial-catalog retry selection: 11 passed.
- Engine-service `--lib graph_catalog`: 46 passed, zero ignored, including
  injected activation retry and stale native-state preservation cases.
- Maintenance selection: 3 passed; control-protocol selection: 6 passed.
- Workspace formatting, changed-crate strict Clippy, engine build, UI typecheck,
  UI boundary check and desktop packaging passed. The existing Vite bundle-size
  advisory was not suppressed.
- Independent review findings at these changed boundaries were addressed.

Precommit cleanup removed unused editor labels/state props and obsolete locale
entries. Its focused UI run passed 75 tests across 5 files; Biome, typecheck,
boundary checks and diff whitespace checks passed. Native tests and the probe were
not rerun for this metadata/documentation cleanup.

Matched live before/after rebuild cost was not measured: the earlier native
initialization failure prevented a controlled comparison under the same conditions.
Ownership/cursor/drop tests provide correctness evidence, not a numerical realtime
performance or latency claim. Live vendor-crash acceptance, broader input/editor
compatibility and a complete leak audit remain outstanding as described above.

## Commit scope

Keep the native transport, protocol, desktop adapter, feasibility example and
owning tests together. Keep node-layout and startup-recovery changes with their
regression tests. Generated binaries, images, logs, caches and local agent
instructions are excluded. This document records validation; it does not imply
that a commit, push or release has been performed.
