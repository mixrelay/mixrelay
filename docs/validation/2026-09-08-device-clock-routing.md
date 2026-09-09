# Device-clock routing repair

## 2026-09-09 clarification

The user explicitly rejected feedback detection, prevention, and automatic correction.
No feedback-specific routing policy, cycle-dependent delay insertion, or protection
feature is in scope. A ToneNari device mixes app playback and graph writes into one
signal, available to all graph readers and OS capture. Ordinary device I/O buffer
ownership and bounded callback scheduling are transport mechanics, not feedback control.
This supersedes feedback-rejection wording in the historical acceptance notes below.

## Contract

- A device owns its nominal sample rate; a processing group follows an actual clock.
- Adding a virtual endpoint must not move unrelated hardware PCM onto a polling worker.
- Same nominal rate is not proof of the same clock. Correct independent clocks exactly once at their boundary.
- CoreAudio aggregate correction and a software bridge must never correct the same boundary twice.
- Preserve DAW/device changes, graph edits, channel maps and persisted settings. Native callback ownership must drain before reclamation.
- No VM work, temporary-directory artifacts, skipped tests or unsupported success claims.
- User-added mandatory completion condition: multiple graphs run concurrently and share app-owned virtual devices. Graph A's produced device audio must be consumable in graph B; switching the viewed graph must not stop A. Device references and lifetime must be independent, with channel/rate changes synchronized across references and cross-graph feedback validated.
- Multiple independently named Virtual IO devices must each expose real system input/output endpoints with their own identity and format; these are not aliases of a singleton HAL device.
- Clarified device lifetime: a Virtual IO exists while at least one node references it
  across all saved/running graphs. Removing the final reference removes the system
  device; there is no independently retained unreferenced device or separate manual
  deletion requirement. Creating a new identity publishes a new system device, while
  adding a reference to an existing identity shares that device. Renaming from any
  graph changes the canonical device name, every reference, and Audio MIDI Setup.
  This supersedes earlier assumptions that unreferenced registry devices survive.
- A physical/application input used by graphs A and B has one canonical capture source and capture owner. Graph-local nodes reference that source and retain independent routing, gain and layout. Removing one reference must not interrupt other readers; device-level format changes are shared, not graph-local mutations. Tests must prove provider deduplication and PCM fan-out, not merely equal device labels.
- Architecture/readability is a mandatory acceptance gate across the implementation, not a language-conversion milestone. Separate device identity/catalog, native property ABI, clock policy, transport ownership, realtime processing, graph composition and control/persistence. Each resource has a documented owner and shutdown order; unsafe/FFI boundaries state thread and lifetime preconditions. No replacement monolithic Rust file, duplicated OS-independent policy, boolean state explosion, lint suppression or increased complexity thresholds in place of design repairs.
- Final independent review must cover the above boundaries and compare preserved functionality against the requirement matrix. Resolve actionable correctness/ownership findings, run the affected regression tests, and document before/after complexity evidence alongside realtime performance. Cross-platform abstraction review is included; macOS implementation and live acceptance remain first, without starting VMs.
- User-mandated Rust-first replacement: replace C implementations wherever Rust can implement the responsibility, including shared PCM transport and device/runtime ownership. Calling an unchanged C implementation from Rust is not completion. Retained C requires a specific ABI/toolchain necessity; the existence of a C operating-system API is not sufficient. Update the current implementation, callers, tests and build inputs together and remove superseded sources.
- Strict pre-release rule: this product has never been released. Previous internal implementations are not compatibility targets. No parallel old/new path, compatibility alias/wrapper, old internal protocol/schema support, obsolete implementation fallback, or backup/archive copy is permitted. This applies across all owned OS backends, HAL, engine, UI, settings, IPC, tests and tooling. Tests coupled to replaced code must be rewritten against the required behavior of the current implementation; they do not justify retaining old code. Preserve required functionality and user work, not previous implementation architecture. Remaining obsolete code is unfinished removal work, not supported "legacy" functionality.

## Work slices

### Required goal additions — confirmed 2026-09-08 18:43

These are required product behavior, not optional follow-up features. The active
goal must not be marked complete until both have implementation and runtime proof.

- [ ] **Reuse a virtual device's produced output signal.** Audio routed into a named
  ToneNari IO must be available as a source reference in the same graph and in other
  graphs, including the user's microphone -> ToneNari IO -> headphones configuration.
  The 2026-09-09 clarification supersedes the earlier read-side selector: application
  playback and all graphs writing to the device form one canonical device signal.
  Graph readers and OS capture consume that same mix, projected to their channel widths.
  Native playback/capture buffers remain separate transport boundaries, not user-facing
  source choices. Do not reject user-authored feedback routes: cyclic processing must
  remain causal and bounded, without adding delay to acyclic routes.
- [ ] **Run graphs concurrently.** Selecting Graph 2 changes the editor's viewed graph,
  not which graph runs. Graph 1's capture, processing and device output continue while
  Graph 2 consumes that output. Share device/capture ownership, retain graph-local routes
  and layout, and release a native resource only when its final user is removed.

Current source evidence: `render.rs::lower_virtual_endpoint` binds `from_os` to
external playback and `to_os` to system capture independently; it does not expose
the produced capture mix as a graph source. `graph_catalog.rs` applies only the selected
document on startup and selection. The composition module alone does not establish
runtime integration. Both requirements are **unimplemented in the live route**, not
passed by the successful engine-only HAL reconnect tests.

Acceptance must cover real PCM in the user's same-graph and A -> device -> B routes,
unchanged Graph A processing during repeated view switches, shared-source deduplication,
independent named devices, edits/removal of one reference without disrupting others,
and bounded processing of user-authored feedback cycles. Verify native ownership/teardown
and actual app behavior in addition to compiler/controller tests.

Device lifecycle acceptance additionally requires N distinct system devices for N
referenced identities, one system device for multiple references to the same identity,
continued PCM after removing a non-final reference, system removal after the final
reference, and name propagation from either graph to the other graph and CoreAudio.

Device-owned sample-rate display and controls belong on the corresponding device node,
not a graph-global footer. Buffer controls must identify their actual device/client/clock
group owner; moving a global control under a node without changing its scope is not a fix.

| Slice | Owner | Status |
| --- | --- | --- |
| Callback-owned hardware + virtual capture clock boundary | windows_native_recovery | Implementing |
| Hardware clock authority and independent HAL/buffer preferences | format_commonization | Implementing |
| Device-targeted control transaction and metadata synchronization | root | Implementing; focused controller tests passed |
| Device-owned settings UI and truthful errors | device_menu_refresh | Implementing |
| Installed native PCM and continuity tests | macos_test_refactor | Implementing |
| HAL source, mixed/independent device paths | root coordination | Not yet verified |
| Concurrent graph execution and real shared-device PCM routing | root | Required; live implementation missing; acceptance unchecked above |
| Independent final code/design review | callback_design_review | Pending final changes |
| Cross-layer responsibility, ownership and readability audit/refactor | root + independent review | Mandatory; in progress |

## Architecture baseline (2026-09-08)

Current source inspection: `ToneNariHAL.c` has 4,530 lines and combines device properties, singleton device/clock state, transport replacement and XPC reconnection. `TNEngineService.c` has 547 lines and combines endpoint memory ownership and IPC; engine `lib.rs` has 1,651 lines and macOS `mod.rs` 1,087 lines. Line counts are inventory evidence, not standalone quality targets. Strict Clippy additionally identified a boolean-heavy callback state and unchecked frame-count narrowing; those need structural fixes rather than exemptions. These findings support the expanded responsibility/readability gate; they do not establish that uninspected modules are correct.

## Reproduction baseline

Before repair, the user recreated ToneNari IO. A read-only measurement at graph revision 52 observed:

- Input: BuiltInMicrophoneDevice mono, gain 0 dB.
- Output: own virtual endpoint capture channels 1 and 2; the same microphone channel feeds both.
- Processing/virtual format: 96,000 Hz; processing buffer: 32 samples.
- No hardware output in this measured revision. Earlier revision 44 also contained headphones, but it is not the measured topology.
- Graph remained unchanged during the 5.160-second measurement.
- Underruns: 211,487 to 222,761 (**+11,274**).
- Overruns: 2,708 to 2,708 (**+0**).
- Discontinuities: 37 to 37.
- API still reported `processingState=running`, `issue=null`.

These are transport starvation counters, not a count of audible clicks. No independent PCM recording was taken during this baseline. A matching-topology after measurement plus native PCM verification is required before claiming improvement. The user continued editing afterward; tests must recheck topology/revision and must not restore an obsolete graph.

The user selected hardware headphones plus virtual capture for the final live comparison. Preserve the microphone gain selected by the user and record it in both measurements.

### Current mixed-route pre-install baseline, revision 97

Read-only remeasurement before installing the Rust/callback changes: microphone
mono at 0 dB feeds virtual capture channel 1 and headphone channels 1/2. Virtual
capture channel 2 is not connected; do not silently add that edge for comparison.
The document stayed identical across 5.203 seconds, at 96 kHz / 32 samples:

- Underruns 2,622,705 → 2,623,333 (**+628**).
- Overruns 7,552 → 7,573 (**+21**).
- Discontinuities 4,689 → 4,709 (**+20**).
- Engine PID 57150, executable path `target/debug/tonenari-engine-service`;
  on-disk SHA-256 `2f79942f54322ce416555953866f9aea2ad2839142aca4bdd324d2fff5dcc9fb`.

This is another pre-install baseline, not an improvement over revision 52:
topology and user activity differ. Counter increments are not independently
captured PCM defects. No user graph, gain, rate or device default was changed.

## Focused validation so far

Controller session-change suite: 9 passed, 0 failed, 0 ignored. Includes hardware processing-rate adoption preserving independent endpoint rate, and endpoint-only observed changes without retuning the processing clock.

Binary compile check passed after the first callback capture boundary implementation. This is not a live-audio acceptance result.
