# Prepared node execution, multichannel scaling and virtual activation

Startup-only follow-up: `2026-09-14-input-startup-admission.md` records the later
one-time physical-input admission correction. The retained startup count below
belongs to this report's earlier build and is not rewritten.

## Scope and ownership

This continues `2026-09-14-callback-owned-src.md`; that earlier failed soak remains
valid historical evidence, not a passing result retroactively reclassified.
The existing runtime, graph-scoped controls, native-device owners and callback SRC
are retained. No second control API, replacement worker, OS default/rate change,
user graph edit or HAL reinstall was introduced in this slice.

Render preparation now owns an immutable `RenderSchedule` and prepared `RouteOp`
channel operations. Block execution visits only applicable source, processing and
sink indices; indices still address the same sampled node controls. Terminal
input-only recorder nodes remain in the sink stage for their mute gate even when
they have no sink kernel. OFF excludes their work and routes; MUTE retains processing
and state and silences the appropriate boundary. Recorder and hosted-processor
regressions run against this common execution path.

`render/kernels.rs` borrows prepared arena, source/sink buffers and processor state;
it does not prepare graphs, own native resources or publish controls. `buffers.rs`
uses safe disjoint slice borrows based on the allocator's input/output port spans.
`routing.rs` owns channel-map classification and execution. Full-width identity
routes are classified once and traverse contiguous samples, without allocating a
channel map; arbitrary maps still preserve ordering and duplicate accumulation.
Gain and mixer processing likewise traverse contiguous slices. Mixer accumulation
preserves the previous input order and `f32` summation's negative-zero identity;
a bit-level signed-zero/cancellation regression failed before that was corrected.

This is a responsibility/execution-cost change, not a claim that file splitting
alone improves the architecture. For the measured 18-node chain and 11-node fanout
plans, phase membership visits fell from 54 to 18 and 33 to 11 per block. The arena
is still cleared in full; arena bytes did not decrease. General plan preparation
and execution coordination still live in `render.rs` and merit further review.
No dependencies, RT allocations, locks, logging, native property queries or new
unsafe DSP paths were added. Existing callback unpublication/drain/reclamation
ownership is unchanged.

## Virtual-client activation

The initial state-classification-only attempt (`activation-fixed`) **failed**:
opening a native capture client still added 32 underruns and one discontinuity,
despite the 30-second PCM window having no exact-zero frames. That result is kept.

The reader incorrectly assumed a newly admitted producer's first packet began at
sample zero. The actual HAL timeline can begin much later. The first publication
now establishes its read origin from `published_through - native_packet_frames`;
unwritten positions before that origin are not read as missing PCM. The synthetic
first-publication regression was observed failing, then passing after the fix.

`InputActivity::Priming` describes only bounded startup waiting for actual native
PCM. Requested graph time advances its budget even if the publisher stalls.
Exhaustion and a stall after readiness remain active-source starvation, not healthy
silence. Intentional OFF discards stale PCM through explicit suspension; unexpected
active epoch replacement reports loss once. Tests separately exercise first
publication, burst activation, suspension, active replacement, missing packets,
rate changes, and startup timeout. Capture-side known-clock priming is also distinct
from a missing or failed clock.

The installed continuity test now takes its counter baseline **before** opening the
capture client and its final snapshot after stopping it. The PCM sample window still
starts after 500 ms reader settling; startup/stop counter regressions cannot disappear
inside that interval. The successful `origin-fixed` run recorded 1,440,256 frames per
channel at 48 kHz, with zero new underruns/overruns/discontinuities. Channel 1 analysis
found zero exact-zero frames/runs and continuous file timestamps. This is OS-visible
virtual capture, not a headphone DAC loopback or an independent microphone reference.

## Native clock evidence

On a detected native timestamp break, the callback retains the count, expected and
actual sample positions, previous/current host timestamps and validity flags in a
fixed atomic snapshot. Control reads it once without spinning and logs it with
device-owner identity; concurrent publication leaves the prior output untouched.
There is no callback formatting or allocation. Rust/C ABI sizes are checked (48
bytes), and publication, pending reads and directional timestamps are tested.

This makes the earlier rare microphone clock gap diagnosable when it recurs.
Instrumentation is **not** proof that the underlying rare native event is fixed.

## Same-condition DSP measurement

Host: Apple M2 Pro, aarch64 macOS 26.5.1 (25F80), Rust 1.96.0
`ac68faa20`, LLVM 22.1.2. Both sides use the unchanged workspace release profile
(one codegen unit, thin LTO), without `RUSTFLAGS` or target-CPU overrides. Serial
runs use the same fixture/input, 500 warmup blocks and 3,000 measured blocks per
case, at 48 kHz. No other benchmark runs concurrently. The normal debug audio app
is running on both sides; OS scheduling/frequency are not pinned.

The chain is input -> 16 unity-gain nodes -> output. Fanout is input -> 8 unity-gain
branches -> 8-input mixer -> output. These tests exercise the common in-memory DSP
graph, **not** native multichannel I/O, SRC, third-party processors, recorders or
meter publication. They are not end-to-end latency measurements.

Raw local evidence under `output/microdropouts-20260914-1105/`:
`scaling-before.json`, intermediate `scaling-after.json`, and frozen DSP source
`scaling-final.json`. The table compares **before to final**, not the best of runs.
All 30 cases preserve zero processing-time heap operations and identical arena sizes.

| Channels | Block frames | Topology | p50 before -> final (us) | Reduction | p99 before -> final (us) |
| ---: | ---: | --- | ---: | ---: | ---: |
| 2 | 32 | chain16 | 1.125 -> 0.375 | 66.7% | 1.417 -> 0.500 |
| 2 | 32 | fanout8 | 1.250 -> 0.375 | 70.0% | 1.458 -> 0.500 |
| 2 | 128 | chain16 | 4.042 -> 1.125 | 72.2% | 5.084 -> 1.416 |
| 2 | 128 | fanout8 | 4.500 -> 0.958 | 78.7% | 6.000 -> 1.292 |
| 2 | 256 | chain16 | 7.875 -> 1.959 | 75.1% | 10.459 -> 2.583 |
| 2 | 256 | fanout8 | 8.709 -> 1.875 | 78.5% | 11.625 -> 2.542 |
| 8 | 32 | chain16 | 3.583 -> 1.125 | 68.6% | 4.917 -> 1.459 |
| 8 | 32 | fanout8 | 4.083 -> 1.000 | 75.5% | 5.542 -> 1.292 |
| 8 | 128 | chain16 | 17.083 -> 5.541 | 67.6% | 32.667 -> 7.459 |
| 8 | 128 | fanout8 | 17.458 -> 4.333 | 75.2% | 38.709 -> 5.750 |
| 8 | 256 | chain16 | 30.042 -> 8.875 | 70.5% | 49.083 -> 11.583 |
| 8 | 256 | fanout8 | 34.792 -> 9.292 | 73.3% | 63.417 -> 14.708 |
| 16 | 32 | chain16 | 6.916 -> 2.041 | 70.5% | 9.875 -> 2.791 |
| 16 | 32 | fanout8 | 7.875 -> 1.875 | 76.2% | 10.209 -> 2.625 |
| 16 | 128 | chain16 | 30.083 -> 8.833 | 70.6% | 59.708 -> 14.542 |
| 16 | 128 | fanout8 | 35.083 -> 9.333 | 73.4% | 93.500 -> 20.000 |
| 16 | 256 | chain16 | 53.541 -> 16.084 | 70.0% | 123.625 -> 30.125 |
| 16 | 256 | fanout8 | 61.958 -> 16.666 | 73.1% | 133.041 -> 28.708 |
| 32 | 32 | chain16 | 17.667 -> 5.084 | 71.2% | 32.458 -> 6.833 |
| 32 | 32 | fanout8 | 17.584 -> 4.334 | 75.4% | 38.959 -> 5.875 |
| 32 | 128 | chain16 | 51.541 -> 16.208 | 68.6% | 93.208 -> 28.875 |
| 32 | 128 | fanout8 | 62.500 -> 17.167 | 72.5% | 133.417 -> 44.667 |
| 32 | 256 | chain16 | 97.375 -> 30.209 | 69.0% | 227.417 -> 63.583 |
| 32 | 256 | fanout8 | 118.000 -> 30.125 | 74.5% | 202.875 -> 53.833 |
| 64 | 32 | chain16 | 33.000 -> 8.917 | 73.0% | 70.542 -> 14.542 |
| 64 | 32 | fanout8 | 36.959 -> 9.375 | 74.6% | 59.625 -> 17.875 |
| 64 | 128 | chain16 | 117.917 -> 29.917 | 74.6% | 278.500 -> 51.833 |
| 64 | 128 | fanout8 | 133.458 -> 30.125 | 77.4% | 205.500 -> 63.542 |
| 64 | 256 | chain16 | 222.208 -> 56.125 | 74.7% | 421.792 -> 217.458 |
| 64 | 256 | fanout8 | 260.291 -> 59.792 | 77.0% | 413.667 -> 110.000 |

The baseline contains one deadline exceedance: 16-channel / 128-frame fanout,
3,746.333 us maximum against a 2,666.666 us deadline. Final has none across 90,000
measured blocks. This finite observation is not a hard real-time guarantee.
Maxima did not improve in every case: 8ch/32 chain 29.583 -> 34.959 us; 8ch/32 fanout
33.625 -> 52.334 us; 16ch/32 chain 42.834 -> 53.583 us; 32ch/128 chain 151.666 ->
172.583 us. Raw files retain all p95/max/arena/deadline values; no outliers removed.

## Validation and live follow-up

Final focused serial checks passed: audio-engine 61 tests (one benchmark explicitly
run separately), audio-io 62 tests and 3 doctests, macOS engine boundary 85 tests,
native device-I/O 26 tests. Strict Clippy for all targets of audio-engine, audio-io,
engine-service and macOS transport passed with `-D warnings`; engine build, their
`cargo doc --no-deps`, workspace fmt check, C-header syntax, soak-script syntax and
diff whitespace checks passed. A function-length lint was resolved by separating
snapshot admission from transfer, without a suppression or raised threshold.

Live final build SHA-256:
`12ec172df5d3494d22d819189ff039326374cb6fce064e1e4171efa0d463289f`.
App-only normal quit stopped both desktop and server, followed by restart from the
same app bundle. The startup status retained one underrun before capture; it was
not reset or attributed to the measured steady-state interval.

`refactor-final` installed capture **passed**: 30,002 ms, 2,813 native callbacks,
1,440,256 frames per channel, graph revision 746 unchanged. The before-open through
after-stop counter window added zero underruns, overruns or session discontinuities.
Both channels had 1,440,256 finite nonzero samples, no exact-zero runs, no malformed
buffers and no native callback timestamp gaps. The two separately analyzed WAVs
likewise have continuous file timestamps and no exact-zero ranges. These checks
do not prove that every possible nonzero click is absent without a known reference.

`refactor-final-soak.jsonl` **passed**: 600.011 seconds / 120 samples,
2026-09-14 03:51:53.216 through 04:01:53.224 UTC. Underruns stayed 1 -> 1,
overruns 0 -> 0, session discontinuities 0 -> 0, issue null. Microphone, speaker
and headphone native starvation/overflow/clock-gap counts stayed zero; epochs
remained 12, 10 and 11 respectively. Graph 1 revision 746 and Graph 105 revision
740 and both complete documents remained unchanged. All native owners stayed
enabled/ready at their original rates, with fresh telemetry throughout.

This was a steady-state observation after the separately tested capture start/stop.
No heavy build, benchmark, control mutation or device restart ran during it; only
read-only telemetry, a short WAV analysis and source/report review. It is not the
same workload as the earlier mixed-use failed soak, nor proof against every rare
native event. The initial underrun is deliberately retained in the baseline.

The integration-test invocation relinked an on-disk test-feature engine after the
normal app launch; the running PID 26587 retained its original executable mapping
through the entire recording and soak. A final normal `cargo build` restored the
on-disk binary to the launch SHA-256 above without restarting the measured process.
No source changed between that launch, capture, soak and final normal build.

## Remaining acceptance / next boundaries

- Native microphone timestamp-gap root cause still requires observed evidence;
  no rare-event fix is claimed merely from a quiet short recording.
- App-start admission still has a retained initial underrun, separate from the
  repaired virtual-client start/stop boundary.
- Physical 32/64-channel hardware, DAC loopback and absolute latency were not
  measured: only built-in mono input and stereo outputs are available here.
- Full mixed-clock 48/96 kHz matrix with processors, recorders, meters and concurrent
  controls remains outside this first DSP scaling measurement. Earlier unavailable
  dedicated native fixtures are not declared passed; neither is the whole workspace
  or Windows/Linux validation.
- Buffer capacity/arena clearing/copy lifetime are next measurable boundaries;
  do not reduce queue capacity or add callback parallelism without occupancy,
  burst and deadline evidence. No memory-reduction claim is made for this change.
- Implementation and read-only self-review were performed by the primary agent
  because the user prohibited subagents; no independent reviewer approval is claimed.

Run the existing ignored benchmark explicitly with a fresh output path:

```sh
MIXRELAY_SCALING_OUTPUT=/absolute/path/new-scaling.json cargo test -q -p mixrelay-audio-engine --release measure_multichannel_render -- --ignored --test-threads=1
```

Use `MIXRELAY_TEST_INSTALLED_ENGINE=1 MIXRELAY_TEST_GRAPH_ID=graph-1` and a fresh
`MIXRELAY_CAPTURE_OUTPUT` prefix with the `macos_installed_continuity`
`installed_mixed_route` test. That test is read-only toward the user's graph.
Recordings and diagnostic output remain local and unstaged; no commit/push was done.
