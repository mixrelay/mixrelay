# Processor mapping ownership and PCM follow-through

Scope: pre-porting cleanup of the current implementation. No Linux/Windows
porting, installation, app launch, or user graph changes are included.

## Mapping boundary

Inspection of the installed memmap2 0.9.11 source confirms that MmapMut has no
inherent as_ptr/as_mut_ptr/len accessors: these calls dereference the mapping
into a whole byte slice. This is not limited to using get/indexing explicitly.
The same dependency's MmapRaw supplies inherent pointer/length access without
creating that whole-region reference. Mapping validity, bounded region access,
and publication ownership still require a narrow unsafe boundary; changing the
backing type alone does not establish those contracts.

The implementation must keep header atomics, state replies, input PCM and output
PCM separate. Audio reads/writes must include the matching sequence check and
acknowledgment rather than expose generally usable sample slices. State reads
must retain the control request guard until copying ends, and stale/duplicate
worker replies must not overwrite an acknowledged state region.

The implementation separates mapping admission, audio transactions, control
transactions and the wire header into one private `shared_transport` module.
The parent factory creates a fresh private file and issues one audio/control
pair. A worker opens its own file handle and acquires a nonblocking exclusive
file lock before mapping. Both worker roles retain that same file lease; each
mapping drops before its lease reference. This lock runs at startup, not in the
audio callback, and adds no dependency or cross-platform implementation layer.

`ParentAudio` and `WorkerAudio` are non-cloneable mutable owners. Sequence checks,
bounded PCM copies and release acknowledgments form a single operation. The
worker retains the consumed input sequence while processing, even if the parent
queues a newer input. Control requests reuse the existing publisher mutex and
retain its guard through the response copy. A separate worker response lock
serializes stale/duplicate checks and publication, never native plug-in calls.
Critical sequence/state fields are private to the transport module. There are
no general-purpose public raw-region accessors or new unsafe Send/Sync impls.

The header and parameter-slot field order, types and representation match HEAD;
the wire version is unchanged. Mapping bounds are checked once at admission.
The remaining unsafe operations are local header initialization/access and
region-scoped copies under those ownership protocols, not per-sample DSP.
The parent timing probe still runs after copying PCM and before its release
publication. A transaction callback preserves that original measurement boundary
without adding a clock read when no measurement is requested.

The standard-library file lock is available before the repository's Rust 1.96
minimum; see [File::try_lock](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock).
The implementation uses a fresh handle, not a cloned handle with unspecified
re-lock behavior. macOS lease tests passed; this is not Windows/Linux runtime
validation.

## Shared audio transport measurement

The baseline executable was captured before production mapping changes, with
the same release-only benchmark added. It measures the actual parent
`HostedExternalProcessor::process` transfer path for 2/8/64 channels and 32/256
frames. A simulated worker performs the same three sequence atomic operations
in both versions. Zero-initialized output is copied, input is copied and a new
request is published. There is no real plug-in DSP, editor capture, worker
process scheduling or file admission inside the timed loop.

Each case submits one initial block, then runs seven rounds of 100,000
iterations and reports the median nanoseconds per iteration. There is no
separate warmup pass. Host/compiler/release profile match the PCM measurements.
Two matched before/after pairs ran sequentially with builds/tests idle. The
benchmark's test-only acknowledgment helper replaces the same inline atomic
operations; its workload is unchanged. Moving its constant declaration to
satisfy release Clippy does not change the workload.

| Channels/frames | Before 1 ns | After 1 ns | Change 1 | Before 2 ns | After 2 ns | Change 2 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 2/32 | 19.573 | 20.397 | +4.2% | 21.274 | 19.472 | -8.5% |
| 2/256 | 78.780 | 80.489 | +2.2% | 79.831 | 79.704 | -0.2% |
| 8/32 | 45.158 | 45.867 | +1.6% | 45.212 | 44.820 | -0.9% |
| 8/256 | 386.644 | 367.649 | -4.9% | 363.106 | 496.712 | +36.8% |
| 64/32 | 393.558 | 354.465 | -9.9% | 377.593 | 289.093 | -23.4% |
| 64/256 | 2569.938 | 2554.157 | -0.6% | 2723.818 | 2574.023 | -5.5% |

These short microbenchmarks show substantial variability, including opposite
directions for 8/256. The +36.8% result is retained, not discarded as noise.
This batch does not claim a general transport speedup or zero ownership overhead.
The structural change is justified by eliminating whole-mapping borrows and
enforcing region ownership. Timing files and baseline/after binaries remain
ignored under `target/test-artifacts/region-cleanup`.

## Direct PCM loop follow-through

The previous safe partial-layout implementation used two strided chunk iterators.
The new loop retains the existing channel order, enumerates only the graph-side
frames, and indexes the already-bounded native slice. No unchecked indexing,
new native unsafe, channel-specific specialization, or benchmark workload change
was introduced. A nonuniform PCM regression covers 2/8/64 channels, packed/planar
layouts, both channel offsets, partial frames, accumulation and untouched padding.

The existing 24-case release workload was run before/after twice, sequentially
with builds/tests idle. The original unsafe executable was also rerun as a
historical reference, not reintroduced as source. All executables/results are
ignored artifacts under target/test-artifacts/region-cleanup. Host/compiler and
workload match the preceding unsafe-boundaries report.

The table uses the second matched run. Case = channels/frames/layout/offset.
Nanoseconds measure validation, one copy and two accumulations together.

| Case | Previous safe ns | New safe ns | Change | Original unsafe ns |
| --- | ---: | ---: | ---: | ---: |
| 2/32/packed/0 | 29 | 33 | +13.8% | 78 |
| 2/32/packed/1 | 113 | 94 | -16.8% | 84 |
| 2/32/planar/0 | 122 | 104 | -14.8% | 83 |
| 2/32/planar/1 | 126 | 114 | -9.5% | 99 |
| 2/256/packed/0 | 106 | 154 | +45.3% | 626 |
| 2/256/packed/1 | 821 | 656 | -20.1% | 696 |
| 2/256/planar/0 | 802 | 660 | -17.7% | 637 |
| 2/256/planar/1 | 832 | 681 | -18.1% | 663 |
| 8/32/packed/0 | 55 | 55 | 0.0% | 297 |
| 8/32/packed/1 | 440 | 341 | -22.5% | 314 |
| 8/32/planar/0 | 494 | 403 | -18.4% | 310 |
| 8/32/planar/1 | 517 | 404 | -21.9% | 341 |
| 8/256/packed/0 | 353 | 349 | -1.1% | 2396 |
| 8/256/packed/1 | 3118 | 2652 | -14.9% | 2271 |
| 8/256/planar/0 | 3237 | 2598 | -19.7% | 2470 |
| 8/256/planar/1 | 3183 | 2644 | -16.9% | 2533 |
| 64/32/packed/0 | 354 | 350 | -1.1% | 2671 |
| 64/32/packed/1 | 3239 | 2542 | -21.5% | 2223 |
| 64/32/planar/0 | 3725 | 3051 | -18.1% | 2380 |
| 64/32/planar/1 | 3752 | 3067 | -18.3% | 2399 |
| 64/256/packed/0 | 4099 | 4030 | -1.7% | 35920 |
| 64/256/packed/1 | 25090 | 21391 | -14.7% | 21030 |
| 64/256/planar/0 | 30632 | 25192 | -17.8% | 23327 |
| 64/256/planar/1 | 30368 | 24610 | -19.0% | 22925 |

Partial/planar cases improved in both runs; the repeated run gives 9.5..22.5%
less elapsed time than the preceding safe implementation. The unchanged packed
fast path varied, especially stereo/256 (first run 171 -> 117 ns, repeat
106 -> 154 ns); no speedup is attributed to that unchanged path. Several partial
cases remain slower than the original unsafe reference, so this is mitigation,
not a claim that all safety overhead disappeared.

## Final validation

- Processor-host debug unit suite: 60 passed; release suite: 61 passed. The
  release-only transport measurement entry and the existing channel-reference
  measurement entry return without timing unless explicitly enabled. Actual
  transport measurements were run separately as described above.
- Direct PCM/callback selection: 15 passed, excluding its measurement entry.
- Nine new transport regressions cover duplicate worker admission, either
  endpoint retaining the file lease, final release, failed admission cleanup,
  overflow/create-new preservation, short/empty PCM, copy unwind, stale and
  duplicate replies, malformed reply length, and concurrent PCM/state traffic.
  The concurrency test exchanges 1,000 PCM blocks and 64 state snapshots.
- Existing editor tests now use distinct parent and worker mappings instead of
  publishing worker replies through a parent mapping. Existing queued-input /
  older-output and editor timeout/lifecycle tests remain active.
- Strict all-target Clippy passed for processor-host, engine-service and desktop.
  Processor-host release all-target Clippy also passed.
- Engine and desktop development builds and processor-host release test build
  passed. Processor-host documentation built; its doc-test target has zero tests.
- Workspace formatting and diff whitespace checks passed. Independent read-only
  review covered the design, implementation and final lint fixes: no new P1/P2
  remains in these changed boundaries. The seven pointer alignment expectations
  apply only to individual casts with admission/alignment proofs, not a module
  or project-wide lint exemption.

The lease/transport regressions use separate mappings in one process. Native
Apple plug-in capability/state tests run in the suite, but the new transport
has not been exercised with a live plug-in worker and physical audio stream.
Miri/sanitizers, installed-app playback and Linux/Windows builds/runtime were
not performed in this batch. No installation, app launch, user graph changes,
commit or push occurred. This completes the stated cleanup batch, not the
project-wide unsafe audit or platform ports.

## Pre-installation check

Before committing the accumulated cleanup for local reinstallation, formatting
and strict all-target Clippy passed again for engine, desktop, macOS transport,
process tap, PCM ring, HAL core and processor host. The HAL, transport, tap,
PCM ring and processor-host test command passed, including its documentation
targets. This is not an all-workspace/live-device success claim.

The broader engine binary run passed 136 tests and failed six live-device
prerequisites: five require `TONENARI_TEST_ENDPOINT_UID`, and the private-group
test requires `TONENARI_GROUP_PRIMARY_UID` and `TONENARI_GROUP_SECONDARY_UID`.
Those variables were not set. No device was selected implicitly to make these
tests pass. The failures are retained as unverified live acceptance, not hidden
by changing tests or reported as passing. Fresh installed-app playback remains
a separate validation step.
