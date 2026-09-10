# Continuous code audit

The implementation source is frozen for root integration. The independent
reviewer read the local AGENTS.md and the full fixed external Rust policy.
Reviews inspect code only; no VM, installation or user graph mutation occurs.
The thread's read-only heartbeat repeats every 15 minutes and reports new
actionable findings, not unchanged status.

| ID | Boundary | Finding | Required evidence | State |
| --- | --- | --- | --- | --- |
| D1 | `native/macos/common/endpoint_transport/demand.rs` | Paired deadline/quantum publication needed a writer-side release fence before payload stores. The fence is now present. | Model/interleaving validation of the actual implementation, in addition to existing native stress tests; independent re-review | Closed for callback pair publication: two Loom models and three native demand tests pass; independent source re-review confirms scope. Concurrent lifecycle reset/retirement is not modeled |
| D2 | `apps/engine-service/src/macos/{mod.rs,capture_schedule.rs,worker_io.rs}` and `macos_pacing.rs` | Coalesced callbacks were consumed before a refused output reservation. Re-review also found truncated large-native/small-DSP windows and stale synchronized status on admission failure. | Actual shared PCM through DSP for native32/DSP128 and native256/DSP32; explicit expiration and rejection counters; HAL-only and mixed admission failure semantics | Closed for these functional findings: latest source independently reviewed; four worker PCM/status tests pass. Live audio acceptance remains open |
| D3 | `native/macos/common/endpoint_transport.rs` | New public snapshot quantum fields lacked precise API documentation. | Latest per-direction quantum versus accumulated credit; units, zero semantics, UNREADY read failure, ABI offsets and independent quantum changes | Closed: independent source re-review plus passing documentation contract and ABI assertions |
| D4 | Source registry and service ownership / PCM boundary | Integration gate exposed an active-MAX old mapping not revoked on replacement, plus ownership/type/complexity violations. Review found unnecessary internal raw-pointer APIs and incomplete public PCM alias contracts. | Retired PCM rejects old writes; current C producer fixture; pinned ownership, bounded retirement, safe slice validation, latest code review | Closed for inspected findings: Pin/reclamation/revocation source reviewed, internal APIs now safe references, public ABI contracts corrected; service ten tests and strict Clippy pass. Not a whole-process memory safety proof |

The inspected Demand and CaptureSchedule modules add no unsafe code, C, heap
allocation, locks or lint exceptions. This is scoped review evidence, not proof
of the entire application's memory safety or real-time correctness.

## Latest root integration evidence

- `cargo clippy -p mixrelay-engine-service -p mixrelay-macos-transport --all-targets -- -D warnings`:
  exit 0 after the final mixed-route rejection test. No lint thresholds or exceptions changed.
- `cargo test --quiet -p mixrelay-macos-transport -p mixrelay-hal-core -p mixrelay-engine-service --lib --bins -- --test-threads=1`:
  latest follow-up exit 0, engine library 139 + binary 91 + HAL 33 + common 68 = 331 passed,
  zero ignored. A separate final model run passed both tests (5.78 seconds, zero ignored).
  The binary includes the five current native C/Rust fixtures and the real private Aggregate
  lifetime/leaks diagnostic. The aggregate test used explicitly observed 48 kHz display
  outputs `1E6D5077-0000-0000-0821-0104B5462878` and
  `1E6DF65B-0000-0000-0921-0104B52F3478` via `MIXRELAY_GROUP_PRIMARY_UID` and
  `MIXRELAY_GROUP_SECONDARY_UID`. It did not change the default device or user graph.
- All four `worker_io` tests, including the mixed-route rejection case, passed in
  the final 329-test run. The previous 328-test checkpoint was rerun after expansion.
- Initial full run failed (not skipped): missing aggregate UID inputs and an old source
  mapping remaining valid at active MAX. The environment was established from live
  CoreAudio inventory; the actual Rust retirement defect was corrected. Both failing
  tests then passed independently and in the full rerun.
- `git diff --check`: exit 0. Instruction files remain local-only and ignored.
- The independent reviewer confirmed D2/D3 from the latest source and root's execution
  evidence, without claiming a separate test run. Review also caught forbidden external
  wildcard imports in `worker_io/tests.rs`; root replaced them with explicit imports.
  After that import-only change, rustfmt check, all four worker tests (zero ignored), and
  the same strict all-target Clippy command passed again. D4 ownership review was assigned
  to the same read-only reviewer; no additional implementation worker was started.

## Behavior and ownership changes

- Worker admission uses the observed native quantum, not mapping capacity or the
  maximum prepared DSP block. Large native windows remain whole when split for DSP.
  Late windows produce explicit overflow/discontinuity events rather than queuing old audio.
- The four worker tests use current EndpointRegistry shared buffers, native demand
  publication and BlockProcessor. Only service discovery is substituted; actual PCM
  read/write and DSP are exercised. Native32/DSP128 observations 32,96,192 deliver all
  32 latest frames; native256/DSP32 observations 256,768,1536 deliver all 256 latest
  frames in eight chunks. Each sequence records two expired-window events.
- There is no controlled pre/post performance benchmark here. The prior loss was
  established by source audit; current sample correctness is measured by exact PCM
  comparison. These numbers do not claim better audible latency or reduced live jitter.
- Worker scratch storage, capture cursor and mutable transport access have one owner;
  no allocation or lock was introduced into processing. HAL-only refusal does not
  consume source PCM; mixed routing continues servicing its other bindings. Refusal
  and late publication failure both clear the synchronized status.
- Registry owners are pinned rather than movable elements behind published pointers.
  Service PCM internals now accept borrowed slices; raw pointer/length conversion stays
  at the C ABI. Source identity validation, lease grant and consumer-range selection have
  distinct responsibilities. The existing fixed application read lead is still unresolved.

## Remaining acceptance

The library/binary checkpoint did not replace the installed engine. The subsequent paired
install, basic live-app check, 32-sample PCM observation and 32/256 counter measurements
are recorded below. Full endpoint-quantum/latency measurements, 256-sample PCM evidence,
controlled before/after validation, comprehensive UI behavior, startup persistence fixes
and the full device-clock/group/global-endpoint goal remain open. No VM was started.

## Follow-up concurrency and ABI audit

- `tests/demand_model.rs` imports the actual `endpoint_transport/demand.rs` algorithm
  with Loom atomics instead of std atomics. There is no copied algorithm, runtime
  feature branch or model dependency in the HAL/engine runtime. The three existing
  native tests remain under `unit_tests/demand.rs`; moving them enables instrumentation,
  not an architectural improvement claim.
- `cargo test -p mixrelay-macos-transport --test demand_model -- --test-threads=1`:
  two passed, zero ignored, 5.68 seconds in the first debug run. The finite cases cover
  one publisher/reader and two contending publishers/reader, accepting only complete
  old/new callback pairs or an explicitly unavailable snapshot. Reset occurs before
  concurrent access. No performance comparison is inferred from model runtime.
- Loom's documented relaxed-memory limitations still apply. These tests do not model
  concurrent outer UNREADY/reset, registry retirement or interprocess mapping lifetime.
  Reference: https://docs.rs/loom/0.7.2/loom/#limitations-and-caveats
- After instrumentation, strict all-target Clippy and common 68 + HAL 33 tests passed.
  The independent reviewer checked the actual algorithm reuse and these evidence limits.
- A subsequent service audit replaced Rust-only raw `matches`/`begin_for` arguments
  with safe references. Null handling remains at the actual C ABI. Public PCM safety
  contracts now specify alignment, allocation extent, lifetime and alias restrictions.
  New tests exercise valid empty/partial/mismatched slices and rejected null snapshots;
  no undefined-behavior overlap fixture is executed.
- The first new service-test compile failed on a missing `std::ptr` import. It was fixed
  before further changes; the rerun passed all ten service tests, zero ignored, followed
  by strict all-target Clippy. Independent re-review found no additional P1-P3 in D1/D4.
- Inspection still finds fixed `READ_LATENCY = 512` in the source registry and matching
  output feedback target plus 256-frame priming in desktop `macos_audio/clock_pump.rs`.
  This path must be redesigned together with its single conversion/drift owner; lowering
  one constant alone would leave the producer/consumer contracts inconsistent.
- Final source rerun: 331 library/binary tests plus two model tests passed; rustfmt
  checks on all follow-up Rust files and `git diff --check` also returned exit 0.
- The installed status endpoint was read without mutation after verification: it still
  reports 96 kHz / 32 samples and `transportLatencyFrames: 512`. Absolute counters are
  cumulative, not a controlled before/after metric. This confirms the running binary
  has not acquired the newly tested transport changes merely because source tests pass.

## Paired runtime verification in progress

- Fresh `cargo build -p mixrelay-engine-service --bin mixrelay-engine-service` and
  `cargo run -p mixrelay-hal-bundle --` passed. HAL ARM64 and x86_64 release builds,
  universal merge, plist validation, code signature verification and actual factory/COM
  probe all passed. Engine SHA256:
  `71ab3d8d07c88393b1e56694f2a97ab6265f30c6e65d6baff5f857347f748baf`.
  New universal HAL SHA256:
  `b4a00ada0cd56a6369e3fd9c749afad243623ee1063a3a6071d0cb8f2b01f72b`.
- The installed-route probe initially failed before observation because its nominal-rate
  selector was incorrectly `nomr`. The local macOS SDK's AudioHardwareBase.h defines
  `kAudioDevicePropertyNominalSampleRate = 'nsrt'`; the test was corrected and rerun.
  Focused strict Clippy and rustfmt checks passed after correction.
- Before restart: graph revision 97, microphone -> virtual capture channel 1 plus both
  headphone channels; 96 kHz / 32 samples; observation 5003 ms. Counters increased by
  436 underruns, 20 overruns and 20 discontinuities. The independent reader received
  938 callbacks / 480256 frames, no malformed PCM or timestamp gaps; selected channel
  had 10304 nonzero samples and a longest exact-zero run of 24192 frames. Acceptance
  failed. Live input was not controlled, so nonzero/zero counts alone do not distinguish
  silence from loss. Graph/revision/rate were unchanged. This is the pre-restart evidence,
  not a passed test or a controlled performance improvement claim.
- Preserved graph-file SHA256 before restart:
  `5593d2c7dacb6a6ac1b4b2385b7fb5893b2de378799a7141b68d10630f5c1991`.
- Authorized installation copied only MixRelayHAL.driver; installed/new hashes match.
  `launchctl kill` could not signal Apple's protected service, so the initial combined
  command stopped before engine restart. A subsequent authorized restart targets the
  freshly observed coreaudiod PID directly and the MixRelay launch job. Restart outcome
  and post-install PCM acceptance still require verification.
- The direct PID/job restart succeeded: coreaudiod changed from PID 35794 to 85251;
  engine changed from 57150 to 85261. The installed driver hash remains the fresh hash.
  Status reports device-owned 96 kHz / 32 samples, virtual nominal rate 96 kHz, all
  six endpoint rate options, and no fixed transport-latency claim (`null`, not zero).
- First post-restart read-only PCM run passed: 5005 ms, 938 callbacks / 480256 frames,
  all 480256 selected-channel samples nonzero, no non-finite samples, zero longest
  exact-zero run, no malformed buffers/timestamp gaps. Underrun/overrun/discontinuity
  increments were 0/0/0, compared with the pre-restart 436/20/20 over 5003 ms at the
  same route/rate/reported buffer. This is a single paired observation under changing
  host load/live microphone input, not a universal latency or CPU benchmark.
  The independent HAL reader's observed callback quantum was 512 frames in both runs;
  the engine's reported 32-sample quantum is not proof that every endpoint uses 32.
- Nodes, channels, gain and all three edges survived restart unchanged. Runtime graph
  revision changed 97 -> 3, and the persisted graph file hash changed accordingly;
  byte-for-byte persistence was therefore NOT preserved. Restart revision semantics and
  the startup `stored session format is unsupported` warning remain to investigate.
- The live MixRelay app's accessibility tree confirms both saved graphs, the same three
  routing nodes/edges, applied state and `96 kHz · 버퍼 사이즈 32`, without an engine
  connection error. This checks the actual installed app, not only Storybook or HTTP.
- The 32/256 buffer integration test initially failed to link: it still declared two
  device-rate functions in the removed C implementation. It now imports the actual safe
  Rust endpoint-service functions directly; no compatibility symbols were added. A remaining
  unnecessary unsafe block was caught by strict Clippy and removed before the next run.
- `MIXRELAY_TEST_INSTALLED_ENGINE=1 MIXRELAY_TEST_MUTATE_SESSION=1 cargo test
  -p mixrelay-engine-service --test macos_installed_format
  installed_hardware_route_has_no_starvations_at_32_and_256 -- --exact --nocapture --test-threads=1`
  passed: at 96 kHz, 32 samples / 5009 ms and 256 samples / 5003 ms each produced zero
  input starvation, output overflow and discontinuity increments. The native test restored
  the original 32-sample setting and checked unchanged topology and own HAL nominal rate.
  This test measures engine counters, not independent 256-sample PCM or acoustic latency.
  There is no pre-install 256-sample measurement; no old implementation was reinstalled
  or preserved to manufacture one. The 32-sample paired observation is the available
  before/after evidence, with the live-input/host-load limitations stated above.
- After buffer restoration, both HTTP and a second live app accessibility inspection
  confirm 96 kHz / 32 samples, applied state and the same three nodes/edges. Final
  all-target strict Clippy and `git diff --check` returned exit 0. The counter test
  does not keep the independent HAL capture reader active; its zero-counter result
  is therefore not substituted for active-reader PCM validation at 256 samples.
- Independent final review checked the local SDK selector and the Rust imports' actual
  UID-based CoreAudio query/setter calls, finding no new P1-P3 or weakened assertions.
  It confirmed the measurement limitations above. These test files are untracked in the
  current worktree, so this is current-source inspection, not a claimed Git diff audit.

## Startup ownership and active-reader 256 follow-up

- `with_graph_catalog_path` previously allocated startup revision from the bootstrap
  processor alone, resetting revision 97 to a small number. Startup now allocates one
  greater than both submitted and every saved graph revision; overflow rejects recall
  before publishing or replacing saved bytes. Regression covers active 97/inactive 90,
  active 3/inactive 90, a larger bootstrap, stale pre-restart apply and exhausted MAX.
  All 31 catalog tests passed. The first new assertions needed correction for canonical
  default layout serialization and the actual `ControlUnavailable` error response;
  neither production validation nor the preservation assertion was weakened.
- Rate-only observed-device adapters now restore the validated client buffer preference
  while following the current device's nominal rate. Startup never calls their rate setter
  to overwrite a DAW's current setting. Missing observation remains DeviceUnavailable
  until the actual clock is known, not a failed settings-write claim. A changed observed
  rate updates saved last-observed settings; a storage failure remains PreferencesUnavailable
  and never causes audio to revert to the wrong rate. Native full-pair negotiation remains
  a distinct existing adapter contract, not an old implementation fallback.
- New regressions cover startup DAW rate adoption without a setter, empty enumeration
  with valid native rate readback, storage failure, missing HAL and invalid saved buffer.
  The initial test fixture used RenderFormat where SessionFormat was required and was
  corrected before rerun. The initial `session_` filter matched only ten control tests;
  the proper `session::` run executed all 37 session tests, including the new ones.
  After removing a reviewer-flagged duplicate eprintln, routing tests 11 and strict
  all-target Clippy passed again. PreferencesUnavailable assertions remain intact.
- The engine setting was changed from 32 to 256 with revision-guarded session API only.
  The existing independent CoreAudio PCM test then passed at 96 kHz / 256 samples:
  5003 ms, 938 callbacks, 480256 selected-channel samples, all nonzero, no non-finite
  samples, no exact-zero run, no malformed buffers or timestamp gaps; counter increments
  0/0/0. Graph revision remained 6 during observation. After proving unchanged topology,
  the same API restored original 32 samples (revision 7 accepted). No recording or
  graph-file backup was created. As before, the reader's actual quantum was 512 frames;
  this validates engine-256 mixed-route PCM, not universal endpoint-256 or acoustic latency.
- Final native integration: 144 engine library + 91 engine binary + 33 HAL + 68 common
  = 336 passed, zero ignored. Fresh engine binary build passed. The independent reviewer
  closed the focused revision/startup findings after current-source re-review and test
  evidence; live restart of this newest engine is the next acceptance step.

## Continuous review checkpoint: engine-only restart remains open

- The latest engine restart preserved both saved graphs and their layout/settings;
  active revision advanced to 91, above the saved maximum 90. The startup settings
  warning was absent. This is not an overall restart acceptance pass: the live status
  still reports `clock_not_synchronized` at 96 kHz / 32 samples, despite zero transport
  error counters. A fresh read confirmed the issue persists.
- The engine log reports a native shutdown exceeding two seconds and callback storage
  retained in quarantine. This requires ownership/reconnection investigation; retained
  storage is not evidence of successful teardown or proof of a permanent leak.
- The prior independent PCM test process handle is no longer available and its terminal
  result was not recovered. No pass is inferred from that missing result or zero counters.
- The existing independent reviewer was resumed for a bounded, read-only HAL reconnect,
  mapping generation, clock-state and teardown audit. No new implementation worker,
  concurrent Cargo run, VM, installation or graph mutation was started for this review.
- The existing 15-minute read-only audit remains active, with duplicate-review suppression
  and notifications only for actionable findings. Findings require current-source review
  and execution evidence before closure. Local AGENTS.md is ignored and not tracked.

## Connection-scoped inventory authority repair

- Independent review identified a P1: the engine's inventory/entry generations restart
  from small values, whereas HAL retained the previous process's high generation.
  Both whole-snapshot and individual-entry gates could silently ignore new mapping FDs.
- The XPC bridge now creates a fresh connection on interruption as well as invalidation.
  It immediately fences queued events from the previous connection and passes a checked,
  non-reused local connection identity to Rust. Rust separately tracks the announced
  connection and the authority of the last validated inventory. Malformed initial data
  cannot reopen the old connection or delete devices. Generation comparisons reset only
  after full preflight; endpoint identities remain available for absent-device removal.
  Registry device IDs, clients, native pending generations and DAW rates are not reset.
- Production-mapping tests cover lower/equal generations with different FDs, pending
  96-kHz requests and delayed old Perform/Abort, malformed new inventory followed by old
  events, authoritative empty inventory, removal before new inventory, and same-mapping
  reconnection without epoch/PCM reset. HAL 37 and common transport 68 tests passed,
  zero ignored. No previous implementation or backup was retained.
- Explicit HAL Clippy gates initially found three test functions at cognitive complexity
  16 (limit 15). Related clock-state assertions were grouped without removing conditions;
  the explicit complexity/length/arguments/type gates then passed. Engine all-target
  Clippy with warnings denied also passed. Whole-workspace formatting initially failed
  on existing files; mechanical rustfmt changes brought fmt check and diff check to pass.
- Fresh arm64/x86_64 release HAL archives and strict C ABI bridge compilation, universal
  link, signature validation and host factory/COM probe passed. The x86_64 slice has no
  claimed runtime validation. Installed restart acceptance remains separate.
- Before installing the repair, the actual 96-kHz/32-sample mixed-route PCM test failed
  again (exit 101): engine PID 92605 / coreaudiod PID 85251, graph revision 91 unchanged,
  5001 ms, 938 callbacks, 480256 frames, channel 1 nonzero 0 / longest exact-zero run
  480256, invalid samples 0, malformed buffers 0, timestamp gaps 0, counter deltas 0/0/0.
  ClockNotSynchronized remained. This recovers a concrete baseline instead of inferring
  success from the earlier missing test handle or from engine error counters alone.

### Installed engine-only reconnect acceptance

- The full engine run initially found one failing C dispatch fixture (144 library
  passed, 90 binary passed / 1 failed). Its assertion still expected reuse of an
  interrupted connection. The test now exercises the current production bridge's
  new-connection contract, 1000 repeated/stale callbacks, cancellation, heartbeat
  inventory and exhausted-counter non-reuse. No old implementation was preserved.
- Added a direct Rust replacement-request regression with positive requests before
  and after the connection change; both old authority and new authority before its
  validated inventory are rejected without modifying outputs. Final HAL 38, common
  transport 68, engine library 144 and binary 91 passed: 341 tests, zero ignored.
  Focused native fixtures also passed, including the updated C dispatch fixture.
  Explicit HAL complexity/length/argument/type Clippy gates and whole-workspace
  formatting passed. The reviewer found no new P1-P3 in either test correction.
- Installed the current universal bundle, verified its signature and restarted
  CoreAudio once to load it. Installed/source HAL SHA256 both equal
  `2315c7e4c072af8f813277a39accf8176ee4e7a563bac63902540492419cf866`.
  Rebuilt engine SHA256 is
  `89d484e8c34d58085f88606091cd274acf8da01efc159a36a2ca55ef943611bd`.
  CoreAudio PID became 98487 and stayed 98487 throughout both engine-only restarts.
- At 96 kHz / 32 samples, engine-only restart changed PID 98497 to 98661. Status
  recovered to Running with no issue. The same independent PCM test passed at revision
  93: 5000 ms, 938 callbacks, 480256 frames and nonzero samples on connected channel 1,
  longest exact-zero run 0, invalid/malformed samples 0, timestamp gaps 0 and counter
  deltas 0/0/0. The before-repair run above had 480256 exact-zero samples and a clock
  error. Microphone content and host scheduling were uncontrolled; this is functional
  reconnect evidence, not an acoustic latency or fidelity benchmark.
- Changed only the session buffer through the revision-guarded API (93 -> 94, 256),
  then restarted only the engine again (PID 99044, CoreAudio still 98487). Saved 256
  was restored without changing the device-owned 96-kHz rate. Independent PCM passed
  at revision 95: 5004 ms, 938 callbacks, 480256 frames/nonzero samples, longest-zero
  run 0, invalid/malformed samples 0, timestamp gaps 0 and counter deltas 0/0/0.
  The independent reader's actual quantum is 512 frames in both runs; this is not
  a claim that every endpoint callback uses the engine's 32/256 quantum.
- Actual MixRelay app inspection after the 256 restart shows both graphs, the same
  microphone/virtual capture/headphone routes, applied state and 96 kHz / buffer 256,
  with no engine error. Both saved graphs, names, gains, connections, layout positions,
  viewports and catalog state compare equal to before installation after excluding only
  each graph document's incremented audio revision. Original 32 was requested back
  through the same guarded API after measurement; final readback is required below.
- The 32 restoration request was rejected with HTTP 409 because new user graph edits
  advanced revision 95 to 105. No forced retry or graph restoration was performed.
  The user subsequently explicitly chose to keep 256; that setting remains in place.
  Later graph edits are user work, not part of the measured unchanged graph checkpoint.
- The independent reviewer closed the engine-only reconnect P1 only for the measured
  96-kHz / engine-32-and-256 mixed-route boundary, based on current-source review and
  root-run evidence (not a reviewer rerun). Global graph sharing, application capture's
  fixed queue, acoustic latency/fidelity and all shutdown-quarantine cases remain open.

## Current N-device and frontend audit checkpoint

The following is an in-progress source checkpoint, not an installed release.

- Required device IDs now cross the format-control API. Device writes/readbacks and
  rollback target that ID, not a session-global endpoint. Callback-pause races preserve
  a newer DAW observation. A failed rollback keeps processing suspended.
- Device definitions and graph-local visible directions are separate. The mandatory
  reference selects OS playback or the graph-produced capture bus explicitly.
- Singleton endpoint preferences/recall implementation was removed. Catalog persistence
  tests cover names, formats, local references and final-reference removal across restart.
- Native endpoint inventory now stages multiple UUID identities as one transaction,
  retaining unchanged mappings and distinguishing graph publication from recovery.
- All-catalog composition exists and has PCM tests, but controller execution, provider
  identity projection and desktop capture ownership are not yet integrated. Do not
  describe graph selection as view-only or cross-graph audio as working in the app.

Root validation:

- Audio I/O: 42 unit tests; audio graph: 23 unit tests; 1 compile-fail doc test passed.
- Engine all-target compile check passed after removing unused singleton bindings and
  correcting the new inventory call sites. No lint exception was added.
- Engine library: 167 passed, 0 failed, 0 ignored on the latest run.
- Engine binary: 92 passed, 5 failed, 0 ignored. The private-group test lacked explicit
  member UID inputs. Four actual CoreAudio fixture tests failed on initialization,
  reconnection or cleanup timeout during the parallel test run. A serialized run with
  verified member identities and diagnosis is still required; these are not passes.
- Initial failed runs also exposed the obsolete device-ID-equals-node-ID constraint,
  an inconsistent fixture capability declaration, and a PCM helper writing frame zero
  twice in the same epoch. The implementation now validates canonical binding identity;
  fixtures declare their actual intended capabilities and assert monotonic PCM continuity.
- No new engine/HAL/app was installed in this checkpoint. No performance improvement
  is claimed: the newly integrated path has not had a controlled before/after measurement.

Independent frontend review found five actionable defects:

1. P2-F1: a session refresh can discard a blocked graph draft and undo history.
2. P2-F2: failed parameter publication hides its error and deletes a newer queued value.
3. P2-F3: suffix-based display-node decoding can change valid runtime identities.
4. P3-F4: test-only singleton graph helpers and hardcoded node-label policy remain in
   production UI code.
5. P2-F5: missing/late processor descriptors can drop authoritative graph edges.

Owners are fixing these boundaries with regression tests. The parameter queue/hook
has 11 passing owner-run tests and a partial independent re-review, but F2 remains open
until visible error, retry, reload and mutation-fence integration is re-reviewed.
All five findings remain open as full user-facing boundaries at this checkpoint.

The independent native review additionally found P1: the direct supervisor constructs
an exclusive renderer reference before callback drain, and resumes callbacks before
that reference's scope ends. The callback owner is correcting this unsafe aliasing
boundary. Installation remains gated on its resolution and re-review.
