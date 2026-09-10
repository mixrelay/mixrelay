# PCM and recovery ownership boundaries

Scope: the project-wide unsafe cleanup's direct macOS PCM boundary and shared
recovery mailbox, including its macOS and Linux callers. This is not a claim
that all project unsafe code is removed, nor a Windows/Linux porting milestone.

Follow-through: [processor region ownership and PCM loop refinement](2026-09-10-processor-region-ownership.md)
records the subsequent partial-layout improvement and completed processor
transport boundary. Measurements and test counts below describe this earlier
batch and are retained as historical evidence.

## Design constraints

- Borrow native PCM for one buffer and one synchronous operation at a time.
  Do not introduce long-lived overlapping mutable channel views, a new rejection
  policy for native in-place buffers, allocation, or sorting in the callback.
- Validate native geometry at admission; keep sample transfer and mixing in
  safe slice operations. The native ABI still owns allocation validity and the
  separation of buffer metadata from writable PCM.
- Make recovery activation require an exclusive, non-cloneable consumer token.
  The token borrows its mailbox rather than owning the final resource reference.
  Unwinding during activation must retire the prepared owner for control-thread
  reclamation, not lose or destroy it on the callback thread.
- Keep Linux's partial resource replacement semantics. Shared API cleanup must
  not turn those operations into whole-set replacement.

## Implemented boundaries

`macos/direct/buffers.rs` is the callback's native PCM admission boundary. It
reuses the existing transport AudioBuffer ABI rather than duplicating it.
Higher-ranked closure borrowing prevents the view from escaping the callback;
each transfer/mix operation borrows only one native PCM extent at a time.
Transfer and mixing loops no longer perform per-sample pointer dereferences.

`RecoveryMailbox::try_consumer` issues one borrowed, non-cloneable
`RecoveryConsumer`; activation requires its exclusive mutable borrow. macOS
binds both publication lanes to the same device backend. Linux retains its
partial capture/device replacement operations and drains the native driver
before dropping callback context, tokens, and the backend owner, in that order.
No compatibility activation API or additional unsafe Send/Sync is retained.

The mailbox still uses narrow internal pointer ownership transfers. A retirement
guard publishes the owner even if the activation closure unwinds. This is not
recovery from process abort: the production release profile uses panic=abort.

The audio-io tests previously owned a custom unsafe global allocation counter.
It conflicted with the shared allocation-counter dev dependency, so its duplicate
implementation was removed. Zero total allocations plus zero net allocations
preserves detection of both allocation and freeing pre-existing storage.

## Measurement protocol

Unmodified-production release executables were captured before replacement in
the ignored `target/test-artifacts/unsafe-boundaries` directory. No copy of the
old production source or fallback implementation is retained.

The direct PCM harness validates actual AudioBufferLists, reads one graph block,
and sums it into the native output twice. It covers 2/8/64 graph channels,
32/256 frames, packed/planar layouts, and channel offsets 0/1: 24 cases. Native
frame offset is one, with a padding frame at both ends. Each case has one warmup
and seven measured passes of 2,000 iterations, plus PCM assertions.

The recovery harness uses one warmup and seven measured passes: 1,000,000 idle
activations and 100,000 publish/activate/reclaim cycles per pass, swapping u64
values. The latter includes control-side allocation/reclamation and is not a
callback-only latency measurement. Consumer acquisition belongs outside the
timed loop. Before/after executions run sequentially, with builds/tests idle,
without extra compiler flags. These are microbenchmarks, not device latency.

Host: Apple M2 Pro, aarch64 macOS, Rust 1.96.0 / LLVM 22.1.2, repository release
profile (one codegen unit, thin LTO). The recovery cycle comparison includes a
test allocator change: the old local counter checked a counting flag, while
allocation-counter also updates thread-local counters outside measurement. Its
publish/reclaim numbers cannot isolate ownership overhead. The idle path does
not allocate. Median idle cost was 3 -> 3 ns; complete cycles were 24 -> 26 ns.

The first safe PCM implementation improved full packed buffers but regressed
planar layouts: 2-channel/32-frame/offset-0 was 81 -> 423 ns and
64-channel/256-frame/offset-0 was 22,253 -> 88,100 ns. These results triggered
replacement of per-frame partial copies with safe channel-strided iteration;
the regression is not omitted from the record.

The intermediate step_by implementation still roughly doubled partial/planar
cost. The final implementation uses channel-offset slices and ordinary chunks,
including the final partial chunk. Two final matched runs confirmed that full
packed buffers are faster but partial/planar buffers remain slower. No unsafe
indexing, channel-specific specializations, or compiler flag changes were added
to conceal that cost. Further partial-layout performance work remains open.

### Final direct PCM measurements

The table uses the second final paired run (`direct-*-repeat.txt`), not the best
sample selected across runs. Case = channels/frames/layout/channel offset.
Negative change means less elapsed time; numbers are median nanoseconds for the
complete validation/read/two-mix workload, not for one sample or end-to-end audio.

| Case | Before ns | After ns | Change |
| --- | ---: | ---: | ---: |
| 2/32/packed/0 | 71 | 27 | -62.0% |
| 2/32/packed/1 | 74 | 108 | +45.9% |
| 2/32/planar/0 | 74 | 120 | +62.2% |
| 2/32/planar/1 | 92 | 131 | +42.4% |
| 2/256/packed/0 | 649 | 108 | -83.4% |
| 2/256/packed/1 | 583 | 783 | +34.3% |
| 2/256/planar/0 | 586 | 799 | +36.3% |
| 2/256/planar/1 | 635 | 817 | +28.7% |
| 8/32/packed/0 | 287 | 54 | -81.2% |
| 8/32/packed/1 | 311 | 419 | +34.7% |
| 8/32/planar/0 | 307 | 479 | +56.0% |
| 8/32/planar/1 | 332 | 488 | +47.0% |
| 8/256/packed/0 | 2450 | 345 | -85.9% |
| 8/256/packed/1 | 2531 | 3061 | +20.9% |
| 8/256/planar/0 | 2531 | 3138 | +24.0% |
| 8/256/planar/1 | 2346 | 3160 | +34.7% |
| 64/32/packed/0 | 2687 | 343 | -87.2% |
| 64/32/packed/1 | 2208 | 3157 | +43.0% |
| 64/32/planar/0 | 2439 | 3726 | +52.8% |
| 64/32/planar/1 | 2440 | 3724 | +52.6% |
| 64/256/packed/0 | 35206 | 4147 | -88.2% |
| 64/256/packed/1 | 19964 | 25071 | +25.6% |
| 64/256/planar/0 | 23094 | 30617 | +32.6% |
| 64/256/planar/1 | 21982 | 30206 | +37.4% |

## Validation

Root executed the following changed-boundary checks, with the final source frozen:

| Selection | Passed |
| --- | ---: |
| audio-io unit tests | 58 |
| audio-io compile-fail documentation tests | 3 |
| Direct PCM/callback tests, excluding the benchmark | 14 |
| macOS resource handoff tests | 3 |
| macOS device boundary tests | 13 |

The recovery benchmark entry returns without timing in normal unit runs; actual
measurements were run explicitly and separately. Direct PCM measurement is also
environment-gated. An initial debug test invocation accidentally included it and
was interrupted; the final 14-test direct selection passed without that timing
entry. Interrupted execution is not counted as a pass.

The device selection includes 512 owner create/retire cycles returning to zero
owners; the macOS leak tool reported zero leaked bytes, with its usual restricted
process visibility limitation. No physical audio device was opened for that test.

Strict all-target Clippy passed for audio-io and engine-service. Engine and desktop
development binaries built; release test executables built; audio-io Rust docs,
workspace formatting and diff whitespace checks passed. Independent read-only
review covered implementation, follow-up fixes and the final chunk iteration;
no new P1/P2 finding remains in these changed boundaries.

Linux audio-io cross-check passed for x86_64-unknown-linux-gnu. Linux engine
cross-check stopped in the native build script because no PipeWire target sysroot
or cross pkg-config configuration is installed. That engine caller has source
review but no Linux build/runtime proof in this batch. No system setup was altered
to bypass the failure. Miri/sanitizer execution of these Rust boundaries, physical
hotplug and installed-app audio validation were not performed.

No app installation, launch, graph mutation, commit or push was performed. The
project-wide unsafe cleanup, especially shared-memory region ownership, remains
unfinished; this report closes only the two stated implementation boundaries and
records the remaining partial-layout performance cost.
