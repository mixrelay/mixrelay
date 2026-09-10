# macOS engine cleanup

Status: accepted direction; implementation in progress, 2026-09-10.

## Scope

Clean up the existing product as a macOS engine. Windows/Linux porting is later
work, not the current implementation or validation priority. Do not lift independent
Windows behavior into macOS or the shared layer. macOS product behavior is the
reference, not the lowest common denominator of the three existing adapters.
Preserve user graphs and state, not superseded internal implementations.

Do not include platform feature implementation, driver installation, VM startup,
or release qualification in a claim that this preparation is complete.

## Boundaries

| Responsibility | Shared owner | Native adapter owns |
| --- | --- | --- |
| Graph identity, routing, mixing and channel projection | graph composition and render plan | observed device/application identity |
| Native resource requirements | `resource_requirements` | streams, process taps, aggregate membership and native selector lowering |
| Session decisions | common session policy | format negotiation and observed clocks, not duplicate decision rules |
| Processing | shared renderer, recorder and hosted processor contracts | callback entry, thread affinity and native transport |
| Lifecycle | common preparation/publication/cancellation contracts | native handle ownership and teardown at the OS boundary |
| Persistence and UI | graph catalog and control protocol | actual supported capabilities, without invented success |

Use existing narrow interfaces and owned configuration through composition.
Do not introduce a universal base backend, inheritance tree, a framework of
single-use traits, or move policy into native FFI merely to share names. Shared
state must not acquire a dependency on macOS, Windows, Linux, or the UI.

## First implementation boundary

`DeviceRequirements` and `ResourceRequirements` are owned description records,
not validated native handles. Their fields are intentionally movable/composable
by adapters; validation and native preparation remain at the existing boundaries.
Extraction expects a validated, composed execution document, preserves canonical
binding IDs and channel widths, and strips only input/application DSP gain from
native cache identity. It neither resolves selectors nor creates transports for
authored virtual-device references. All composed virtual transports are retained.

Replaced local extraction in macOS device specs, Linux device/resource specs, and
Windows graph/format preparation. Linux's duplicate `DeviceSpec` was removed.
Windows graph and format changes now use the same extractor. This removes repeated
policy, not the separate native resource owners. Public API growth is two data
records and their extraction/empty-query methods; no new dependency, trait, C,
unsafe block, thread, lock, or runtime compatibility branch was added.

The macOS translation now temporarily owns input/output vectors before constructing
its native ordered map. This control-thread allocation tradeoff is explicit; no
performance improvement or realtime latency reduction is claimed. Callback and
retirement code are unchanged. The new tests are correctness evidence, not a
matched performance benchmark.

Windows cross-check also exposed pre-existing dead code from macOS-only inline
editor input consumption. Gate the consumer type/methods for macOS or their tests;
keep the shared memory layout and producer protocol unchanged. This does not add
Windows/Linux inline editor support.

## macOS maintenance ownership

The macOS reclaimer and device-format monitor now belong to explicit control-side
worker guards in `macos/maintenance.rs`. This is a macOS lifecycle boundary, not a
cross-platform worker framework. No Windows code or policy was imported.

Previously the reclaimer was detached and held an upgraded exchange across its
100 ms sleep. It now releases upgraded owners before parking; its guard requests
stop, unparks and joins the thread. The reclaimer guard is created before the
audio-worker guard. Shutdown therefore stops the format monitor, finishes control
persistence, joins audio, then joins the reclaimer. Early-return cleanup follows
the same reverse declaration order. Existing work/error retry intervals remain
100 ms for reclamation and 250/1000 ms for format observation.

The reclaimer can perform native recovery, not just free memory. Joining waits for
that in-flight operation; this change does not promise bounded native-call latency
or a 100 ms shutdown limit. No callback, PCM, format policy, C/unsafe boundary,
dependency, or native resource ownership protocol changed.

Structural evidence: the two maintenance thread lifecycles now have one owned
stop/wake/join implementation; the detached reclaimer path is removed. Runtime
performance improvement is not claimed, so no throughput/latency benchmark was
substituted for lifetime tests. Five tests cover parked-worker wake/release,
in-flight join, later-startup failure, deterministic per-pass owner release and
worker exit when the exchange no longer exists.

Validation of this macOS slice: maintenance 5, session clock 5, device handoff 2
and retirement 3 tests passed (15 total; zero failed or ignored). Changed-crate
strict Clippy, formatting and engine build passed on macOS. Independent final
source review found no remaining blocker in this boundary. No live audio run or
engine replacement was performed, and this is not completion of whole-code cleanup.

## Explicit startup and controller test preparation

The explicit document constructor used by macOS initializes directly from the
already prepared graph. It no longer calls the default-document constructor and
then overwrites its graph. Default selection remains solely in the convenience
constructor. There is one initialization body, no new public API or dependency,
and no native preparation or status query in the explicit path.

The same counting fixture was run against the previous constructor and its
replacement: explicit-startup status calls changed from 1 to 0. The previous
constructor failed the zero-call assertion; the replacement passed. This is a
deterministic call-count comparison, not a startup-time or audio performance claim.
Document, format, meter bindings, exchange ownership and mismatch rejection are
also checked.

Expanded checks exposed stale test setup: several helpers tried to prepare a
second controller plan while the submitted one still awaited audio acknowledgement.
One test-only helper now activates the actual pending processor and returns it for
PCM assertions. It checks both pending and activated revisions. The old duplicate
helpers are removed; graph-view tests assert that selection submits no DSP plan.
Failed restore tests now require the saved document/catalog to remain available,
the native restore to stay pending and saved bytes to remain unchanged. The macOS
application-format fixture uses the supported exact-bundle selector, not descendants.
No production capability or failure guard was relaxed.

Before these fixture repairs the full library run reported 189 passed/13 failed.
Restoring the previous constructor reproduced the six endpoint persistence failures
and the separate macOS application-selector failure, excluding the constructor
change as their cause. After repair, all 202 engine-service library tests and both
macOS application-format tests passed, with zero ignored tests. Strict Clippy and
the engine build passed. These are controller/owned-fixture checks, not live audio
or whole-project acceptance.

## File persistence versus session policy

Graph catalogs no longer depend on the audio-session settings module to write
files. The private `persistence` module owns application paths and staged JSON
replacement; `SessionPreferences` owns only the bounded version/backend/format
document contract. `StagedWrite` is consumed on commit or cancels on drop. The
previous staged type and helper exports are removed, not aliased or duplicated.

The existing same-directory create-new/write/sync/rename sequence, owner-only
permissions and destination preservation are unchanged. Temporary names now
identify generic document writes. Post-commit directory-sync and cleanup warnings
use structured logging. Neither a new dependency nor C/unsafe code was introduced.

Session pause, native negotiation/rollback, preferred-format publication and error
policy are unchanged. In particular, an already-observed external clock change
still proceeds when preference storage fails; user-initiated changes still reject
or roll back on the corresponding storage failures. A disabled store is not
reinterpreted as a failed store.

Six direct file-fixture tests cover commit, cancellation, partial serialization
failure, rename failure, independent staging owners and Unix permissions. All 208
engine-service library tests passed with zero failures or ignored tests; strict
Clippy, build and documentation generation passed on macOS. Independent source
review found no remaining blocker. No live engine replacement was performed.

This is a dependency/ownership cleanup, not a performance optimization: graph
catalog's dependency on session settings goes from one to zero, while the file-I/O
algorithm is retained. No latency, throughput or crash/power-loss durability gain
is claimed from fixture tests.

## Processing lifetime and startup admission

`session::processing` now owns worker lifetime and the suspension handshake by
value. Five lifecycle atomics no longer belong directly to the session/status
object; `ProcessingLease` borrows only this owner. Worker states are interpreted
through an enum. No additional Arc, heap allocation, lock, unsafe block, or OS
dependency was introduced. Transfer counters, clock observations, format decisions
and the failed-reconfiguration latch remain with session policy.

Inspection still cannot acknowledge a running callback. macOS direct processing
drains the installed renderer before the outer block-boundary checkpoint. A fresh
ticket is required for every suspension; the control wait retains its 1-second
deadline and 1-ms cadence. A timeout returns to the session's guarded resume, so
failed rollback cannot accidentally unlatch suspension. Six focused tests and
the complete 40-test session selection passed. This changes ownership, not the
audio algorithm; no CPU or latency improvement is claimed.

`ControlObservers` owns the format monitor before control persistence in field
drop order. Normal finish, cancellation of a polled future and unwinding all join
the monitor before final control-owner release. The reclaimer outlives audio as
before. Three tests prove this ordering with an actual `ControlMaintenance` owner;
the maintenance selection now has eight passing tests. Process-global native
service/dispatch singletons are not made session-scoped by this change.

macOS reserves its loopback control listener before any native initialization,
catalog preparation or audio-worker creation. Bind failure therefore rejects a
second startup before those side effects. The admitted listener is moved into
the server and released on startup failure. Two ephemeral-loopback tests confirm
occupied-address rejection, preservation of the existing listener, and release
after failure. They do not start a second installed engine.

## Removed obsolete singleton diagnostics

Removed the unbuilt singleton client-buffer helper/header and its standalone
test/probe: four tracked C/header files, 333 lines. Their only references were
one another and an outdated diagnostic recipe. They selected the obsolete
`io.mixrelay.audio.endpoint.default`, not a current canonical device identity.
The historical buffer-semantics finding is retained, but the dead build recipe
and present-tense singleton instructions are removed. Git retains recovery history;
no user graph, device, recording or cache was deleted.

The current Rust client-buffer owner remains authoritative. Its added malformed
snapshot test covers missing identity/current size, non-finite or reversed range,
invalid settable flag and unavailable observation. All 13 buffer-control and
transaction tests passed. This is not a claim of equivalent CoreAudio ABI coverage
or removal of every obsolete native diagnostic.

## Remaining cleanup

1. Session lifecycle and suspension ownership are separated; format negotiation
   versus status reporting still needs review. Preserve prepare/commit/cancel and
   rollback; do not import Windows semantics to unify unused adapter paths.
2. Startup admission and worker shutdown order are covered. Process-global native
   service/dispatch ownership remains a separate lifetime boundary.
3. Audit duplicated macOS graph/native preparation using composition only where
   it removes an actual repeated policy or clarifies a resource owner.
4. Separate controller policy from OS runtime implementation at real dependency
   boundaries. Move packages only where ownership/dependency separation follows.
5. Audit unavailable plug-in recovery, native ownership and C/unsafe boundaries.
   A missing plug-in must preserve graph state and ports; that remains unresolved.

Existing Linux single-endpoint and Windows LAB/fixed-buffer restrictions are not
macOS or shared product requirements. Their independent cleanup and port feature
implementation are outside this slice.

### Native inventory still requiring work

Independent read-only review followed actual engine/desktop build inputs. Native
code is not yet reduced to indispensable ABI glue:

- `MRProcessTap.m`: process selection, native refresh/unregister/drain/retry and
  callback/listener lifetime still remain in Objective-C. Format/input validation
  and peaks belong to Rust `process_tap_input`; admission ownership, epoch
  invalidation/commit and one-time retirement now belong to `process_tap_lifecycle`.
- The device queue now belongs to `native/pcm-ring` (see below), not the Linux
  native directory. The app-capture ring's partial-read/drop-frame semantics stay
  separate from the device queue's epoch/prefill/whole-read contract.
- `MRDeviceIO.c`: stream-format admission, context allocation and epoch/priming
  remain in C. Rust `device_io_lifecycle` owns the registration pair and
  create/start/unregister failure transitions. Callback buffer validation,
  projection and clock publication also belong to Rust; C retains native buffer
  clearing and OS callback dispatch.
- `MRAggregateGroup.c`: CF object ownership and native property collection remain.
  Rust `aggregate_group` now owns the immutable member geometry, freshness,
  active membership, drift readback and channel binding decisions; the existing
  engine planner remains the sole admission/primary-selection policy.
- `MRApplicationSourceTransport.c` is now only a 20-line XPC blocks adapter.
  Mapping/FD ownership and producer lease/epoch/publication live in Rust, sharing
  the registry's `application_source_mapping` owner instead of duplicating it.
- `MixRelayHALBridge.c`: Rust `control_session` now owns reconnect authority and
  heartbeat cadence. COM, XPC/dispatch handles and Blocks remain in C, including
  checked queue/timer allocation before callback publication. `MREngineService.c`
  is much closer to a narrow dispatch/XPC trampoline; that does not justify the
  other remaining C policy.
- Other installed/live diagnostics still contain singleton assumptions, including
  the currently built `MRDeviceIOLive.c`. Removing the four unbuilt buffer files
  does not resolve those fixtures.

The next lifecycle slice's ownership, failure-injection tests, actual uninstalled
universal HAL build, and matched tap callback measurements are recorded in
[native lifecycle extraction](../validation/2026-09-10-native-lifecycle-extraction.md).

## Integrated validation before callback-clock extraction

Final selected tests on the frozen macOS source and lockfile: 433 passed, zero
failed or ignored. This is the following explicit selection, not all-workspace or
installed-device acceptance:

| Selection | Passed |
| --- | ---: |
| engine-service library (catalog, persistence, composition, session) | 214 |
| audio-engine / audio-graph / audio-recorder libraries | 36 / 26 / 32 |
| control-protocol / control-server libraries | 7 / 17 |
| macOS callback, transport, maintenance and startup tests | 56 |
| macOS pacing / buffer-control and transaction tests | 8 / 13 |
| native callback/HAL contracts (ASan and UBSan) | 1 |
| processor-host editor-surface tests | 8 |
| patched VST3 / CLAP library tests | 13 / 2 |

Changed-crate all-target strict Clippy, engine build, desktop type-check, engine
documentation generation, repository formatting and diff whitespace checks passed.
The dependency audit initially failed on three unused direct `thiserror` edges in
the patched plugin adapters. They were removed without suppressions; at this
checkpoint the lockfile changed only those three edges and the rerun passed. No
package version changed.
LV2 native linking/execution was not performed on macOS.

`MIXRELAY_TEST_ENDPOINT_UID` is unset. Installed-device reconnect/allocation tests
requiring an explicitly selected duplex test device were not run; no arbitrary
user device was selected to make the suite pass. No live app/engine replacement,
graph edit, commit or push was performed. Native audio continuity, entire C-policy
removal and missing-plugin durability remain outside this validation result.

Independent read-only review found no remaining blocker in this batch's ownership,
startup, persistence, removed-diagnostic or dependency boundaries. Remaining native
and product gaps above are not marked completed by the passing selected tests.

## Callback-clock policy and publication in Rust

`callback_clock` in the existing macOS transport crate now owns timestamp
validity, single-stream continuity and bounded atomic publication. The C header's
implementation is removed. Its remaining inline shim projects CoreAudio's
`AudioTimeStamp` into native scalar fields; it does not implement continuity or
invent synchronization between devices. Both `MRDeviceIO.c` and `MRProcessTap.m`
use this implementation, including counter reads when a snapshot is unavailable.

Storage remains caller-owned: 64 bytes, alignment 8, initialized before IOProc
registration and retained until callbacks/readers drain. The snapshot is 32 bytes,
alignment 8. C and Rust both assert the ABI. Only the serialized callback mutates
the writer cell; concurrent readers access atomic publication fields. There is
no blanket `Send`/`Sync` implementation, heap owner, new runtime dependency, lock,
retry loop or callback logging. Unsafe code is restricted to the five native
entry points and their documented storage/aliasing contracts.

Reconnect resets only writer priming after the previous IOProc drains. Published
observations and cumulative discontinuities survive. Invalid timestamps unprime
once; half-frame tolerance, host monotonicity and wrapping counters retain their
existing meanings. A contended snapshot makes one attempt and leaves output
untouched on failure. The old C implementation is not retained as a fallback.

Eight Rust tests cover ABI initialization, invalid/edge timestamps, continuity,
reconnect, failed reads/null pointers, integer rollover, concurrent publication
and allocator traffic. The 10,000-call native-entry/snapshot test measured zero
allocations/frees. The existing 100,000-publication C pthread fixture exercises
the actual cross-language calls. The Objective-C tap-lifetime fixture now belongs
to Cargo's native contract selection and links the same Rust archive; its C/ObjC
AddressSanitizer and UndefinedBehaviorSanitizer instrumentation remains enabled.
The stable-Rust archive itself is not sanitizer-instrumented.

### Matched callback measurement

The unchanged C harness `native/macos/common/tests/MRCallbackClockBench.c` was
built before replacing the header, then linked to the new implementation. On the
same arm64 Mac (Apple Clang 21.0.0, Rust 1.96.0), both runs used `clang -O3
-std=c17 -Wall -Wextra -Werror`, contiguous 128-frame timestamps, 2,000,000 record
calls per pass, one full warmup pass and seven measured passes. The Rust archive
used the repository's normal Cargo release profile; no custom compiler flags or
CPU target overrides were used. Benchmarks ran sequentially without concurrent
build/test jobs. These are callback-record costs, not audio end-to-end latency.

| Statistic (ns per record) | Previous inline C | Rust native entry |
| --- | ---: | ---: |
| Median | 8.523 | 8.534 |
| Minimum | 7.894 | 8.425 |
| Maximum | 8.546 | 8.636 |

Measured passes, in order: C `[8.546, 8.523, 7.894, 8.367, 8.526, 8.509, 8.523]`;
Rust `[8.611, 8.636, 8.534, 8.472, 8.555, 8.515, 8.425]`. Median difference is
approximately +0.13%, below the observed variation; no speedup is claimed. The
earlier independent C baseline median was 8.498 ns. This is a responsibility/C
removal change, not a performance optimization.

Reproduce the new implementation's measurement from the repository root:

```sh
cargo build --release -p mixrelay-macos-transport
mkdir -p target/test-artifacts/callback-clock-benchmark
clang -O3 -std=c17 -Wall -Wextra -Werror \
  native/macos/common/tests/MRCallbackClockBench.c \
  target/release/libmixrelay_macos_transport.a -liconv \
  -framework CoreAudio -framework CoreFoundation \
  -o target/test-artifacts/callback-clock-benchmark/after
target/test-artifacts/callback-clock-benchmark/after
```

The pre-change binary is a local ignored build artifact, not an archived source
implementation. The comparison's old algorithm is available from Git history.

### Validation of callback-clock extraction

The final macOS selection passed 311 tests with zero failures or ignored tests:
transport library 77, transport concurrency models 2, engine-service library 214,
native callback/HAL contracts 1, macOS device boundary 13 and desktop clock pump
4. The native selection includes 1,000 tap teardown/recovery cycles and 100,000
concurrent clock publications. The device fixture's 512 create/retire cycles
returned the owner count to zero; its isolated `leaks` run reported zero leaks.
Those numbers do not establish leak freedom for arbitrary native failures.

Actual engine and desktop linking builds passed, as did all-target strict Clippy
for transport, native test support, engine and desktop, transport documentation,
repository formatting and whitespace checks. The independent Objective-C
syntax-only check retained the stricter conversion/shadow/pedantic warning flags.
Intel macOS Rust type-check and C syntax/ABI assertions passed; no Intel executable
was run. The only new dependency edge is dev-only `allocation-counter`, already
present in the workspace lockfile; no runtime dependency or package version changed.

Independent source review found no blocker in the clock's ownership, ABI, reset,
publication or native fixture integration. No installed-device test, live engine
replacement, graph mutation, commit or push was performed. This closes the clock
policy extraction only; the remaining native inventory above is still open.

## App-capture PCM ownership in Rust

`capture_ring` now owns the app-capture PCM vector, immutable geometry, producer/
consumer cursors and drop counter. `capture_ring::ffi` is the narrow native
entry/copy boundary; the old `MRAudioRing.c` algorithm and test-only allocator ABI
are removed. Desktop, Cargo native fixtures and the standalone smoke link recipe
all link the current Rust owner. No extra runtime dependency, thread, lock,
platform abstraction, Windows policy or previous-implementation fallback was added.

The PCM vector uses `try_reserve_exact` before initialization. The separate native
owner uses fallible global allocation, not abort-on-OOM `Box::new`; destruction
recovers the exactly matching global allocation and drops the vector. Failure at
either allocation returns null, and failure after PCM preparation releases it.
Both stages have deterministic failure injection in private Rust tests rather
than a public native allocator API. This is not a forced system-wide OOM test.

Only the serialized IOProc writes and only the pump/refresh consumer reads. Raw
copies access cursor-owned spans of `UnsafeCell<f32>`; no whole-ring or whole-slab
mutable borrow overlaps another thread, and there is no `unsafe impl Sync`.
The native session still retains the ring until callback, consumer and stat-reader
access has drained. An unregister failure still retains owned resources.

Power-of-two capacities 1..2^20, 1..64 channels, whole-block write rejection,
drop-*frame* accounting, planar interleaving, immediate partial reads and untouched
output tails are preserved. Cursor/counter arithmetic wraps explicitly. One
diagnostic contract is tightened: `available_frames` is capacity-bounded even if
its two cursor loads span concurrent progress. It is not a transactional snapshot
and is not used to grant PCM ownership.

The device `MRPcmRing` was deliberately not folded into this queue. Its half-
capacity write limit, drop-*event* counters, requested/published epochs, prefill
and whole-read-with-silence behavior are different responsibilities. Its C
implementation remains an open cleanup item, not a shared policy to impose on
app capture.

### Capture-ring performance evidence

The same C harness (`MRAudioRingBench.c`) was compiled before deleting the C
implementation and then linked against the Rust replacement. Both used Apple
Clang 21.0.0 `-O3 -std=c17 -Wall -Wextra -Werror` on the same arm64 Mac; the Rust
archive used Rust 1.96.0 and the unchanged repository release profile, without
custom `RUSTFLAGS` or CPU target overrides. Each case used capacity 1024, 20,000
write/read pairs, one full warmup pass and seven measured passes. Benchmarks ran
sequentially, without concurrent test/build work. Every case checks exact output.

Initial measurement exposed slower eight-channel planar copies: Rust emitted a
scalar channel loop while Clang unrolled it. The final implementation splits wrap
once per block and copies fixed eight-channel groups plus the remaining channels.
It also omits empty second interleaved copies. These are the production copy paths,
not benchmark-only variants; no benchmark input or compiler option was changed.

Below are the medians (ns per write/read pair) of two complete sequential before/
after comparisons after the final production change. Lower is better.

| Channels / frames / input | C #1 | Rust #1 | C #2 | Rust #2 |
| --- | ---: | ---: | ---: | ---: |
| 2 / 32 / interleaved | 14.525 | 17.312 | 15.329 | 14.271 |
| 2 / 32 / planar | 39.837 | 74.610 | 41.379 | 35.748 |
| 2 / 256 / interleaved | 64.960 | 64.746 | 58.562 | 58.990 |
| 2 / 256 / planar | 320.279 | 264.652 | 293.017 | 241.671 |
| 8 / 32 / interleaved | 38.860 | 39.481 | 35.754 | 34.948 |
| 8 / 32 / planar | 80.544 | 86.306 | 74.904 | 77.677 |
| 8 / 256 / interleaved | 262.973 | 272.050 | 244.002 | 255.488 |
| 8 / 256 / planar | 698.981 | 706.202 | 649.323 | 628.371 |
| 64 / 32 / interleaved | 360.552 | 380.006 | 342.752 | 361.644 |
| 64 / 32 / planar | 781.302 | 789.296 | 729.210 | 773.371 |
| 64 / 256 / interleaved | 2471.471 | 2584.750 | 2961.279 | 2654.610 |
| 64 / 256 / planar | 6277.300 | 6526.192 | 6781.475 | 6549.860 |

The stereo/256 planar median decreased about 17% in both comparisons. Several
other cases remain slightly slower or change direction between runs; the first
stereo/32 planar run varied from 37.350 to 81.533 ns. No overall speedup, universal
no-regression guarantee or end-to-end audio latency improvement is claimed.

Reproduce the current path without opening a device:

```sh
cargo build --release -p mixrelay-macos-transport
mkdir -p target/test-artifacts/audio-ring-benchmark
clang -O3 -std=c17 -Wall -Wextra -Werror \
  -I native/macos/application-capture/include \
  native/macos/application-capture/tests/MRAudioRingBench.c \
  target/release/libmixrelay_macos_transport.a -liconv \
  -framework CoreAudio -framework CoreFoundation \
  -o target/test-artifacts/audio-ring-benchmark/after
target/test-artifacts/audio-ring-benchmark/after
```

The previous executable is an ignored build artifact; no old source implementation
is retained beside the replacement. Seven Rust tests cover both allocation failures,
invalid geometry, all 1..64 channel widths with bit-exact planar wrap/partial reads,
integer rollover, oversized/null-channel handling, bounded stats and allocator
traffic. 10,000 callback write/read pairs allocate/free nothing; creation/destruction
returns both owner and PCM allocations to zero outstanding bytes.

The native C fixture additionally transfers 170,000 interleaved frames concurrently
for each of 1/2/8/64 channels and checks every frame/channel (680,000 frames total).
It uses the actual Rust FFI, not a duplicate queue implementation. Planar bit tests
are sequential, not a claim of planar concurrency instrumentation. The existing
Objective-C fixture still covers 1,000 teardown/recovery cycles. C/ObjC fixture
code retains ASan/UBSan; the stable-Rust library is not sanitizer-instrumented.

Final selected macOS checks passed 318 tests with zero failed or ignored:
transport library 84, transport models 2, engine-service library 214, native
contracts 1, device boundary 13 and desktop clock pump 4. Engine and desktop
linking builds, changed-crate all-target strict Clippy, formatting, transport
documentation, dependency audit and whitespace checks passed. The standalone
process-tap smoke executable linked with the strict native warning flags, but was
not run. Intel macOS Rust type-check and native C syntax checks passed; Intel
execution remains unverified.

Structural result: 200 lines of C queue implementation removed, native entry
points reduced from nine to eight by removing the allocator hook, fallible owned
storage separated from native raw-copy entry points. The Rust implementation is
not claimed to have fewer lines: explicit safety contracts and failure handling
are part of this boundary. The pre-existing Rust shared-buffer protocol is not
reused here because it has overwrite/epoch semantics rather than this SPSC contract.
No new dependency or lockfile change was needed for this slice.

Independent in-progress and final source review found no blocker in allocation,
ownership, span copying, channel grouping, statistics or build integration. No
running app/engine replacement, capture permission request, user graph change,
commit or push was performed. The removed C source remains recoverable in Git.

## Device PCM ownership and epoch publication (2026-09-10)

The shared device queue is now the dependency-free runtime crate
`native/pcm-ring`. The 136-line `MRPcmRing.c` implementation is removed. Its header
and native contract fixture move out of the Linux directory; the header contains
only opaque storage, diagnostics and eight entry points. Both existing native
consumers and their build/test recipes use the Rust owner. This corrects a shared
dependency, not a Linux port or an import of Windows policy into macOS.

The native stream embeds 120 bytes of aligned opaque ownership state; PCM retains
its anonymous `mmap` allocation rather than switching to heap storage. `Storage`
alone retains the mapping extent and unmaps it on drop. Empty/failed initialization
is clearable, with clear allowed only after callbacks and observers drain. Producer
and consumer mutate distinct `UnsafeCell` state; no `unsafe impl Send/Sync`, mutex,
callback allocation or whole-mapping mutable slice is introduced. Queue policy,
mapping ownership and raw FFI copies have separate modules. Existing standard
`io::Error` handles mapping failures; no new runtime error framework is needed.

An epoch and its starting cursor previously used separate observations. The reader
could combine an earlier epoch with the next publication's start. A sequence-checked
atomic snapshot now either accepts both values or rejects that one read without
spinning. Producer acknowledgment still precedes zero-length/overflow rejection;
discard cannot rewind a cursor, and a restart observed after copying silences the
entire output. The two-quantum prefill and no-prefill entry points retain their
different clock-adapter contracts. No feedback-loop detection or correction is added.

The fixed native layout/mapping constants support only 64-bit x86-64/AArch64 on
macOS/Linux. Other architectures fail explicitly instead of silently using the
wrong anonymous-mapping flag. This restriction was caught and closed by independent
review; it is not a claim that Linux or Intel audio execution was verified.

Ten Rust tests cover invalid/zero initialization, injected mapping failure,
non-power-of-two wrapping and bit-exact PCM for every 1..64 channel width, stalled
and changing epoch publication, full-queue zero-frame acknowledgment, discard,
and a restart injected into the actual read path after the PCM copy. Across 6,400
callback pairs the allocator counter reports zero allocations. In an isolated
child, 64 separate 16MB mappings are dropped; `mprotect` verifies each mapping's
starting page is then unmapped (`ENOMEM`). This checks the starting page, not
every page; source review confirms `munmap` receives the original full extent.
The first test attempt incorrectly used Darwin `mincore` as an unmapped-address
test; it returns success for that case. The test was corrected to the documented
`mprotect` error boundary and rerun, not skipped.

The existing native C fixture now calls Rust and remains part of the Cargo native
contract gate, including quantum/channel/wrap/overflow/silence/epoch scenarios and
100,000 concurrent stereo transfers. ASan/UBSan instruments the C fixture, not
the stable-Rust static library. No sanitizer coverage of Rust internals is claimed.

### Matched device queue measurements

Same arm64 macOS host, Apple Clang 21 `-O3 -std=c17`, Rust 1.96 release defaults,
no target-CPU flags or `RUSTFLAGS`. The unchanged C harness uses capacity 16,384,
one preloaded block, 20,000 write/read pairs per pass, one warm-up and seven timed
passes; final PCM is checked. Creation/destruction is outside the timed interval.
Each before/after executable ran sequentially, with no concurrent Cargo/test or
benchmark jobs. The old executable remains only as an ignored build artifact;
the old source is not retained alongside Rust.

Medians in nanoseconds per write/read pair, two complete comparisons:

| Channels / frames / prefill | C #1 | Rust #1 | C #2 | Rust #2 |
| --- | ---: | ---: | ---: | ---: |
| 2 / 32 / no | 17.650 | 26.400 | 19.400 | 15.950 |
| 2 / 32 / yes | 17.700 | 16.450 | 18.200 | 16.300 |
| 2 / 256 / no | 59.950 | 63.750 | 67.700 | 66.600 |
| 2 / 256 / yes | 65.150 | 66.600 | 64.600 | 61.800 |
| 8 / 32 / no | 49.450 | 45.650 | 43.000 | 45.850 |
| 8 / 32 / yes | 47.850 | 47.900 | 45.850 | 43.400 |
| 8 / 256 / no | 336.400 | 391.300 | 356.950 | 371.700 |
| 8 / 256 / yes | 384.900 | 399.800 | 363.650 | 387.200 |
| 64 / 32 / no | 382.400 | 402.250 | 372.900 | 371.950 |
| 64 / 32 / yes | 369.150 | 417.650 | 373.750 | 356.000 |
| 64 / 256 / no | 3444.400 | 3188.000 | 2724.200 | 2303.800 |
| 64 / 256 / yes | 2616.650 | 2238.250 | 2261.550 | 2297.600 |

Results vary; notably 8-channel/256-frame medians are slower in both comparisons
(about 4–16% without prefill and 4–6% with prefill). This change is an ownership
and epoch-correctness refactor, not an overall speedup. No universal no-regression
or end-to-end latency improvement is claimed. Current-path reproduction:

```sh
cargo build --release -p mixrelay-pcm-ring
mkdir -p target/test-artifacts/device-ring-benchmark
clang -O3 -std=c17 -Wall -Wextra -Werror -I native/pcm-ring/include \
  native/pcm-ring/tests/MRPcmRingBench.c target/release/libmixrelay_pcm_ring.a \
  -ldl -lm -o target/test-artifacts/device-ring-benchmark/after
target/test-artifacts/device-ring-benchmark/after
```

### Device queue validation and residual work

The final frozen-source checks passed 328 tests, zero failed or ignored: PCM 10,
macOS transport 84 plus 2 models, engine library 214, native contracts 1, device
boundaries 13, desktop clock pump 4. The standalone C PCM fixture also passed.
The existing device-retirement test completed 512 create/retire cycles and reported
zero owner allocations remaining; macOS `leaks` reported zero leaks in that child.
That heap check is separate from the PCM mapping test described above.

Engine/desktop linking builds, all-target strict Clippy for engine/desktop/native
test support/PCM, formatting, PCM documentation, dependency audit, shell syntax
and whitespace checks passed. PCM library/tests type-check on Intel macOS and
x86-64 Linux, and the C fixture/benchmark pass Intel macOS syntax checks. Neither
cross-target was executed; the full Linux engine/native lab is unverified because
this Mac has no configured PipeWire cross sysroot. No lab/VM was launched.

Independent in-progress/final review closed the ownership, epoch, mapping failure,
stats and architecture-boundary findings against current source. The feature
matrix also contained three already-moved source paths; those now point to the
actual graph-state checkpoint, graph catalog and device catalog owners. Matrix
generation/check passes without changing feature status or release gates.

The native inventory above remains open: device buffer projection, aggregate-group
policy, process-tap lifetime and HAL bridge control policy still require focused
cleanup. This slice does not complete the overall refactor. No live app/engine
replacement, driver installation, user graph change, commit or push was performed.

## Validation of the first boundary

The subsequent four-way native policy extraction and its matched measurements
are recorded in [native policy extraction](../validation/2026-09-10-native-policy-extraction.md).
This moves device projection, aggregate readback, tap input/peaks and source
transport ownership to Rust together. It does not finish native session lifetime,
registration/priming, or HAL bridge policy cleanup.

- Common requirement tests: 4 passed; graph composition: 20 passed; catalog: 47
  passed; macOS requirement-to-native mapping: 1 passed; editor input transport:
  8 passed. All executed selections had zero failures and zero ignored tests.
- macOS changed-crate strict Clippy and engine build passed. Formatting passed.
- Windows x86-64 MSVC binary/test type-check passed after correcting the editor
  consumer boundary. This is not a Windows executable run or audio test.
- Windows all-target strict Clippy remains failed in existing native worker/render
  functions and native test helpers (function length and argument-count limits).
  The shared extraction boundary does not waive those gates or claim Windows
  readiness. The editor consumer's test now verifies every event field instead
  of suppressing unused-field errors on non-macOS targets.
- Linux x86-64 check stopped in the build script because this Mac lacks a
  configured PipeWire cross sysroot/pkg-config setup. No fake native library,
  skipped build step or host-target substitution was used. Linux remains unverified.
- Independent source review found no remaining blocker in the changed boundary.
- No live engine replacement, graph mutation, commit or push in this refactoring
  step. Entire-port readiness, native audio continuity and leak freedom are not
  established by these focused tests.

## Complete owner conversion and project-wide unsafe follow-through

The file-level batch is recorded in
[native owner conversion](../validation/2026-09-10-native-owner-conversion.md).
DeviceIO and aggregate device C implementations are removed. Process tap lifetime
belongs to a dedicated Rust crate that depends on shared transport; the engine
and HAL do not acquire its AppKit dependency. Engine dispatch and HAL control
now own their native resource lifetimes in Rust, with only SDK COM, XPC Blocks
and Cocoa/AppKit construction left in the native compiler adapters.

Unsafe review covers every owned OS/backend, not just these files. Plugin channel
reference construction now uses safe inline storage and current-layout admission;
the whole-project raw PCM view and single-consumer capability boundaries remain
explicit work. Mechanical replacement of a narrow raw shared-memory region with
whole-mapping slice borrowing is not an accepted unsafe-removal strategy.
The clock bridge removes redundant dispatch/geometry work without new unsafe.
Matched measurements include regressions and variability; neither C removal nor
a passing test suite implies universal speedup, whole-project safety completion
or Windows/Linux port readiness.
