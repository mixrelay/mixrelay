# Callback-owned SRC and remaining engine work — 2026-09-14

Follow-up implementation and measurements are in
`2026-09-14-node-render-scaling.md`. Results below describe the earlier build and
retain its failures; they are not overwritten by later results.

Status: live microphone/headphone continuity improved and the final 30-second
recording passed. The user reported no audible clicks. This is **not a complete
engine acceptance**: the longer observation retained a virtual-client activation
failure and one subsequent microphone clock gap, and 32-channel full-engine
performance has not been measured.

## Changed boundary

Independent physical-device SRC now executes synchronously in that device's
native callback. The separate PCM polling loop is removed. The remaining device
thread supervises connection, prepares format changes, and emits diagnostics;
it does not transfer PCM. Callback state owns the prepared converter and scratch
storage. Replacement and teardown unpublish that state and drain announced
readers before mutation or release; native unregistration follows guard teardown.

The callback neither logs nor acquires the control mutex, allocates scratch, or
queries native properties. Converter errors cross the boundary as atomic codes
and are formatted on control. Tests cover reader drain, exclusive callback
ownership, control-lock independence, OFF/ON, replacement, and no heap activity.

Output resume now invalidates the previous queue epoch before publishing fresh
PCM, and admits native reads only after actual converted samples prime the
transport window. This fixes the one native starvation observed per ON in the
first callback build. No synthetic priming samples are published as valid PCM.
The obsolete worker-transfer ABI names and polling module are removed, with
native callers and the C header updated together.

Diagnostics distinguish device/clock identity, native and graph queue depth,
deadline-facing starvation, overflow, clock gaps, converter correction, reset
cause, transfer counts, and callback work time/budget. Formatting is off realtime.
Source-equivalent pending-buffer time is not physical end-to-end latency.

## Same-route measurements

All recordings used the app-owned debug engine, 48 kHz / 32-frame graph processing,
96 kHz mono built-in microphone at 0 dB, 48 kHz built-in speaker output, and the
existing stereo MixRelay IO route into 96 kHz headphones in the second graph.
No OS default, native rate, endpoint configuration, or graph topology was changed.
Ambient acoustic input was not an identical prerecorded stimulus. Recordings were
serial; no benchmark or separate heavy build ran during the capture interval.

Local artifacts: `output/microdropouts-20260914-1105/`; none uploaded or staged.
The native recorder captures the OS-visible virtual bus, not the headphone DAC.

| Approximately 30-second observation | Polling + timing baseline | Callback SRC | Final resume fix |
| --- | ---: | ---: | ---: |
| Artifact prefix | `before-scheduling` | `after-callback` | `after-final` |
| Frames per channel | 1,440,256 | 1,440,256 | 1,439,744 |
| Engine underrun increment | 544 | 0 | 0 |
| Engine overrun increment | 0 | 0 | 0 |
| Engine discontinuity increment | 0 | 0 | 0 |
| Channel 1 exact-zero frames | 8,530 | 0 | 0 |
| Channel 1 exact-zero runs | 11 | 0 | 0 |
| Longest exact-zero run | 2,211 frames / 46.0625 ms | 0 | 0 |
| Native callback timestamp gaps | 0 | 0 | 0 |
| Invalid/malformed PCM | 0 | 0 | 0 |
| Installed continuity test | FAIL | PASS | PASS |

Final recording: 30,001 ms, 2,812 native callbacks, revision 740 unchanged; both
channels contain 1,439,744 nonzero finite samples. The slight frame-count difference
comes from the bounded capture window and is not a timestamp gap. The test samples
its status baseline after a 500 ms capture-start settling interval. Consequently
this PASS does **not** cover client activation; the longer trace does cover it.

The timing-only baseline observed worker elapsed-work maxima of about 7.102 ms
(headphones) and 3.578 ms (microphone), and wake overshoots up to 5.924/7.166 ms.
These include OS preemption and are not pure CPU time. Callback invocation
frequency differs, so per-call means are not an apples-to-apples CPU speedup.
The structural improvement is removal of the additional PCM wake dependency;
actual PCM and deadline-counter changes above establish the observed result.

Twenty external microphone gain changes retained the open Options panel and
returned `requiresGraphReload: false`; gain was restored to 0 dB. Ten final
headphone OFF/ON changes restored ON with no new underrun, overrun, discontinuity,
or native starvation. Intentional activity resets remain visible in diagnostics.

## Longer observation and limits

`final-soak.jsonl` retains the entire ten-minute observation, including failures;
counters were never reset. Final result: **FAIL**, 600.011 seconds / 120 samples,
underruns +35, overruns +0, session discontinuities +1; both graphs stayed at
revision 740. Repeated failure entries preserve the same cumulative differences
at later samples and are not additional distinct dropout events.
Native microphone/speaker/headphone queue counters and
epochs remained stable through the observed virtual-client activation. At
03:07:49.568 UTC, overall underruns had increased by 32 and discontinuities by 1.
The preceding HAL trace changed from stopped epoch 1 to running epoch 2 at
03:07:45.692835 UTC as the native recording client started. Physical-device
starvation and clock-gap counts stayed zero. This localizes the follow-up to the
virtual-client activation/reporting boundary, but does not yet establish whether
every reported event corresponds to lost audible PCM. Do not suppress the counter
or label the complete observation a pass.

At 03:09:59.740 UTC, a second event added 3 overall underruns. Microphone native
clock gaps and starvation each increased from 0 to 1, and its bridge epoch changed
13 -> 30 with a recorded clock-reset cause. Headphone counters remained unchanged.
The measured PCM work maxima did not increase at that event; this does not prove
the native callback was scheduled on time. Its native timestamp/scheduling cause
is unresolved. This event was outside the final recording window. The soak also
overlapped serial developer validation and capture-client start/stop, so it is a
mixed-use observation, not an isolated idle or repeatable load benchmark.

The microphone also had one startup overflow before this observation, retained
as the initial baseline. Startup and virtual-client activation still need explicit
acceptance. No physical DAC loopback, independent simultaneous microphone
reference, or end-to-end latency measurement was performed. Positive user
listening feedback and virtual-bus recording do not replace those measurements.

## Validation

Frozen-source focused checks: shared clock bridge 14 passed; callback ownership
and metrics 6 passed; activity 3 passed; direct rendering ownership/metrics 6
passed; bridge fixtures 3 passed (one explicit serial benchmark not run).
Final native device-I/O suite: 25 passed. Strict Clippy (`--all-targets -- -D
warnings`), engine build, and diff checks passed. Format check, documentation for engine-service/audio-io/
macOS transport, C-header/benchmark syntax check, and soak-script syntax passed.

An earlier broader macOS device invocation encountered six unavailable dedicated
fixture cases: a missing primary output UID for a private aggregate and five
live CoreAudio cases requiring `MIXRELAY_TEST_ENDPOINT_UID`. They are not passed
or silently skipped. Two additional synthetic fixture failures caused by the new
callback admission gate were repaired, and their focused tests passed. No full
workspace, cross-platform, or external-hardware test pass is claimed.

## Next scoped work, not yet implemented

1. Close virtual-client activation, startup accounting/PCM behavior, and the
   remaining native microphone timestamp-gap recovery first. Add start/stop/
   restart measurements that include the transition itself and native timestamp
   deltas rather than inferring cause from work time alone.
2. Separate render-plan preparation, block execution, DSP kernels, and control
   publication by responsibility and resource lifetime. `render.rs` currently
   contains these distinct responsibilities; file splitting alone is not enough.
3. Consolidate node execution/control policy while preserving OFF (skip work and
   signal), MUTE (keep state/processing, silence the boundary), stable external
   controls, and graph-scoped removed-node rejection. Do not invent a second
   runtime/API or conflate hardware-clock ownership with node identity.
4. Establish full-engine 2/8/16/32/64-channel baselines before optimizing. Vary
   actual route fan-out/mixing, same/independent clocks, 48/96 kHz, 32/64/128/256
   frames, processors/recording/meters, and concurrent controls. Run serially
   with the same profile/input/graph before and after. Report p50/p95/p99/max
   callback work, deadline misses, PCM errors, allocations, memory, and thread
   count. Distinguish synthetic tests from actual multichannel hardware evidence.

Concrete candidates, not measured bottleneck claims: `process_loaded_block`
clears the whole render arena; three phases scan all nodes; `apply_routes` loops
over frames and each mapped channel; native rings, bridge scratch, converter
FIFO/scratch, and destination rings copy PCM across boundaries. One 16,384-frame
float FIFO is 2 MiB at 32 channels; native input/output rings plus the bridge FIFO
alone therefore reserve roughly 6 MiB per 32-channel bridge, before scratch and
filter state. Queue capacity is not target delay. Measure occupancy and burst
requirements before changing capacity, and measure copies before adding SIMD or
parallel work to a deadline-sensitive callback.

The converter already has 1/2/8/16/32/64-channel waveform/isolation and no-allocation
tests. Those do not establish full-graph throughput or all-node scalability.
