# Linux independent sample-clock drift investigation — 2026-09-07

**Paused by user instruction on 2026-09-08 KST.** The user identified memory
pressure from concurrent VMs and prioritized completing macOS first. All timing,
dropout and comparative-performance results below are **loaded-environment
observations, not qualified performance measurements**. Even clean individual
cases do not establish a reproducible unloaded baseline. No drift capability was
enabled from this evidence. All owned drift processes had exited and been reaped
before the stop instruction; the Ubuntu VM is safe to suspend. No further guest
job was started after that instruction.

This experiment uses an independently paced synthetic RTP source, the installed
PipeWire RTP receiver/adaptive resampler, and the production ToneNari native
capture stream plus native graph callback. It does not use physical hardware or
change a system PipeWire service, default audio device, user graph or application.

The sender's sample counter and absolute monotonic timer are independent of
PipeWire callbacks. A matching PipeWire graph clock ID downstream is therefore
not offered as evidence that the external sample clock matches that graph clock.

## Exact implementation boundary

`TNDriftSender.c` sends 240-frame stereo S16BE RTP packets containing identical
1 kHz, 0.25-amplitude sine samples to **127.0.0.1 only**. Nominal rate is 48 kHz;
absolute deadlines inject a requested ±2,000 ppm rate error. Packet timestamps
increment by exactly 240 samples. Each run records actual packet/frame counts,
first/last send timestamps, observed ppm, and maximum/RMS scheduling lateness.
Because sine phase advances against the nominal sample counter, the independently
paced sender's physical tone is 1002 Hz at +2000 ppm and 998 Hz at −2000 ppm.
A correctly clock-aligned output should approach those frequencies, not 1000 Hz;
without compensation the receiver plays nominally 1000 Hz while its buffer
grows or drains. The analysis reports error against the measured sender rate.

The private PipeWire 1.0.5 daemon loads its installed official
`libpipewire-module-rtp-source`. In constant-latency mode, its
[RTP audio implementation](https://github.com/PipeWire/pipewire/blob/1.0.5/src/modules/module-rtp/audio.c)
feeds ring-fill error into `spa_dll_update` and publishes its reciprocal correction
through `SPA_IO_RateMatch`. The
[module documentation](https://docs.pipewire.org/page_module_rtp_source.html)
distinguishes this mode from direct timestamp mode, which assumes a synchronized
driver and does not use that DLL. This experiment uses constant-latency mode.

The recorded experiment linked immutable production `TNPipeWire.c` and the
then-current C PCM queue. The current reproduction script links `TNPipeWire.c`
with `native/pcm-ring`'s Rust static library; that replacement has not been rerun
in this Linux lab and does not inherit the experiment's runtime acceptance.
`TNDriftProbe.c`
it opens a native mode-3 capture against the fixture's exact registry serial and
reads it from `tn_pw_driver_open` callbacks. It measures PCM, finite/stereo
integrity, abrupt sample jumps, silent runs, actual native clock/format snapshots,
and ToneNari ring counters. The Rust engine/controller, desktop UI and a real
application are not running in this probe.

The control changes only RTP source `resample.disable` from true to false.
Both cases retain identical sender ppm/duration, format, graph rate/quantum,
latency, code and instrumentation. PipeWire still computes a DLL correction when
resampling is disabled; its log alone does not prove that the correction affected
PCM consumption. Buffer trend and PCM observations are therefore required too.

The private daemon uses 48 kHz / 128 frames and a private D-Bus/WirePlumber 0.4
session with hardware monitors disabled. Every run owns its runtime/config/state
directories and tracks/reaps only its own daemon, policy, bus, sender and probe.
UDP port occupation is checked before use. All files remain as evidence.

## Initial run retained as unsuccessful

The first matrix used 90 seconds and 50 ms target latency, excluding the first
20 seconds from PCM measurement. At +2,000 ppm, actual sender rates were +1,998.5
ppm without resampling and +1,999.7 ppm with resampling. Final-30-second RTP buffer
growth fell from **99.53 to 12.31 frames/second**, while the enabled DLL correction
averaged **+1,752.7 ppm**. This shows the installed adaptive path changing real
sample consumption; it is not yet a complete steady-state result.

It is **not an end-to-end pass**: sender scheduling lateness reached 107.93 ms
and 43.32 ms, respectively. The enabled run retained three ToneNari ring
starvations, 35 abrupt sample jumps and 256 stereo mismatches. The original probe
grouped those stereo mismatches into its `invalid` count and returned exit 1.
The matrix stopped and all failed output was preserved. Later instrumentation
separates stereo mismatches from non-finite samples; the original failure is not
relabelled as successful.

The follow-up matrix uses the same on/off comparison with a 250 ms RTP target
latency and 120 seconds per case, excluding 40 seconds of PCM warm-up. Its numbers
must not be combined with the first matrix to claim an improvement under identical
conditions.

## Follow-up matrix: complete, but continuity not qualified

The 120-second, 250-ms matrix completed all four cases using the same v5 native
snapshot and `drift-fixtures-v3.tar.gz`. Each probe returned exit 0 for finite,
stereo-identical, nonzero PCM; that exit condition is **not** a continuity pass.
After 40 seconds of warm-up every case had zero RTP overrun, underrun, packet
sequence or resynchronization messages. Native ring starvation remained.

| Sender / correction | Measured ppm | Tail buffer slope, frames/s | DLL correction ppm | Ring starvation | Maximum silent run, frames | Abrupt jumps |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| +2000 / disabled | +1998.547 | +279.268 | +2394.108 | 7 | 512 | 69 |
| +2000 / enabled | +2000.012 | +97.744 | +2358.178 | 6 | 640 | 50 |
| −2000 / disabled | −1999.426 | −57.449 | −2392.474 | 7 | 256 | 25 |
| −2000 / enabled | −2000.069 | +62.909 | −1060.404 | 8 | 384 | 12 |

The positive paired buffer-growth reduction is 65.0%, but the enabled buffer
still drifts. The negative pair reverses buffer slope and does not establish
settled correction. These results cannot be presented as a stable end-to-end
drift pass. All four reported zero non-finite samples, stereo mismatches and ring
overflows. Sender maximum lateness was respectively 74.28, 99.55, 21.52 and
42.20 ms, so matching requested ppm does not imply jitter-free delivery.

Raw files are retained in `drift-v2-evidence.tar.gz`; the reproducible host-side
summary is `drift-v2-analysis.json`. Native callback tracing is a separate
follow-up with the v9 snapshot (including the safe SPA position-pointer handoff),
not an unlabelled replacement of the v5 results. The native probe is compiled
with `-O2`; it does not contain Rust DSP code and Rust development-profile
optimization cannot explain this probe's results.

## Boundary trace on the newer v9 native implementation

The lab-only `TNDriftTrace.c` includes the unmodified v9 `TNPipeWire.c` and wraps
its ring read/write calls. Separate single-writer trace banks are allocated and
prefaulted before streaming; callbacks perform no trace allocation, locking or
file IO. Every row records the actual monotonic timestamp, PipeWire clock ID,
position/time, frame count, returned count and both ring cursors. The later PCM
trace additionally records per-block stereo mismatches and amplitude. Files are
written after both native callbacks have stopped.

The first 120-second trace (`tonenari-drift-lab-GsIACc`) measured actual sender
+2000.777 ppm, maximum lateness 12.478 ms and RMS lateness 0.300 ms. After warm-up,
RTP logged no underflow/overflow/sequence/resync fault. Producer callbacks supplied
3,833,472 frames; the consumer requested 3,834,624. Their **1,152-frame difference
equals nine 128-frame zero returns**. The largest producer callback interval was
18.850 ms despite a nominal 2.667-ms quantum. Its graph position advanced by
768 frames at one interruption. Other starvation windows at sender-relative
50.690, 79.051, 79.123 and 79.152 seconds contain skipped producer positions.
This directly locates the silence at unequal native callback delivery exhausting
the two-quantum reserve, rather than equating packet-rate error with an RTP
underflow. The trace does not establish which host/guest scheduling component
caused the delayed callbacks; no xrun counter was exposed by this instrumentation.

The PCM-boundary trace (`tonenari-drift-lab-SvIxDh`) reproduced one 128-frame stereo
mismatch. Consumer cursor **3,933,184** maps exactly to producer cursor
**3,933,184**, which already contained the same 128 mismatched frames and
0.24999778 maximum channel difference. The producer event repeated its preceding
graph position, only 0.0645 ms after the preceding callback. Thus this block was
already inconsistent at PipeWire's delivered input buffer; the ToneNari ring did
not introduce that mismatch. This run had substantially worse scheduling
(143.738-ms maximum producer interval, 106.242-ms maximum sender lateness), so
its larger starvation count is not attributed to a code regression.
Unlike the earlier v9 trace, this worse-scheduled run also logged an RTP buffer
overrun at monotonic 93728.684746 seconds (96,170 frames against a 96,000-frame
limit). The mismatched producer block arrived at 93729.551445 seconds. Their
proximity is retained as context, not asserted as proof of the module's internal
channel-level cause.

Evidence archives `drift-trace-v4-evidence.tar.gz` and
`drift-trace-v5-evidence.tar.gz` retain the logs, native trace CSV and exact private
configuration. `drift-trace-v4-analysis.json` and `drift-trace-v5-analysis.json`
are reproducible using `analyze-native-trace.mjs`; its mismatch correlation uses
sample cursors, not merely nearby wall-clock timestamps.

The v9 native C SHA-256 is
`e46e1f0473919ddb57607350689d3872b7db2f10fa21fdc7a24e33a69e74a4b8`;
header SHA-256 is
`2cd261b5ae8a3a34521c41cb54f04cc31886ff63ca43fde8656dfd18ac79fb23`.
These immutable files predate the subsequently developed lifecycle-epoch change.

The minimal private configuration does not load `libpipewire-module-rt`; these
results do not measure a production session with RTKit/realtime priority. Debug
logging and the VM scheduler are also part of the retained test conditions.
The follow-up uses a 1024-frame private quantum to separate deadline sensitivity;
it does not change the system quantum or erase the 128-frame evidence. PipeWire
1.0.5's official RTP implementation initializes DLL bandwidth with a fixed
128-frame value, so increasing the actual quantum can lengthen convergence in
wall time. A short larger-quantum run must not be assumed to have reached a
steady correction merely because its PCM is continuous.

## Larger-quantum controlled comparison

The v9 native snapshot and `drift-fixtures-v6-quantum.tar.gz` were used for another
four 120-second trials, still with 250-ms target latency and 40-second warm-up.
Only the owned graph quantum changed to 1024 frames; within each signed pair,
only `resample.disable` changed. The fixture archive SHA-256 is
`77d83a4352d64e5961e3e45b46f4a072db8e6ad0a144758661735dcb11fbd8a4`.

| Requested ppm / correction | Actual ppm | Output Hz | Tail DLL ppm | Tail fill slope, frames/s | Ring starvation | Jumps | Max sender lateness |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| +2000 / disabled | +1993.363 | 1000.000 | +542.307 | +95.169 | 0 | 0 | 34.05 ms |
| +2000 / enabled | +1992.262 | 1000.488 | +545.657 | +243.357 | 0 | 2 | 109.09 ms |
| −2000 / disabled | −1999.889 | 1000.013 | −547.182 | −96.095 | 0 | 0 | 29.34 ms |
| −2000 / enabled | −2007.253 | 984.965 | +415.754 | +106.856 | 16 | 62 | 1132.52 ms |

Both disabled controls produced exactly 3,840,000 measured frames, no channel
mismatch and no abrupt jump. The enabled positive case remained free of native
ring starvation but missed six graph quantums and had two jumps. Its frequency
shift is consistent with the measured partial DLL correction, not full lock.
The disabled negative source reached a four-message RTP underrun near the end,
consistent with its −96-frame/s depletion; this remains a module-boundary fault
even though the native PCM counters did not report a gap during that capture.

The last case cannot measure steady drift: producer and consumer callback
intervals reached **1118.552 and 1125.362 ms**, respectively, concurrently with
the sender's 1132.522-ms lateness. RTP then reported three packet-sequence jumps
and resynchronizations around monotonic 94392.614–94392.632 seconds. Its native
ring returned 44 zero blocks after warm-up (45,056 frames), including re-priming,
and the rendered longest silent run was 12,986 frames. Neither adaptive SRC nor
a two-quantum native reserve is a remedy for a one-second scheduling interruption.
This is an observed shared execution interruption, not evidence identifying a
particular host or guest scheduler component as the cause.

No native production fix is inferred from these traces: starvation follows
callback-delivery deficits, and the separately observed channel mismatch was
already present in PipeWire-delivered PCM. The experimentally demonstrated
boundary is an installed RTP adaptive path with partial rate correction, plus
deadline-sensitive native delivery in this non-realtime-priority VM fixture.
An unconditional `driftCorrection=true` or settled ±2000-ppm continuous-audio
qualification is **not justified** by these runs. The baseline and all failure
artifacts remain available in `drift-quantum-v6-evidence.tar.gz`, with the computed
summary in `drift-quantum-v6-analysis.json` and final-case callback trace analysis
in `drift-quantum-v6-negative-trace.json`.

## Final bounded RT diagnostic and resume handoff

Before the stop instruction, one five-second v7 diagnostic loaded the official
`libpipewire-module-rt` into the owned daemon with requested priority 20. The
private configuration retained `support.dbus=false`. PipeWire explicitly logged
that regular realtime scheduling was unavailable and Portal/RTKit fallback was
disabled. `thread-policy.txt` showed both daemon/probe data threads in `TS`, with
no realtime priority. This confirms the isolation policy's runtime boundary;
it does **not** prove that Ubuntu or the user's normal PipeWire session cannot
obtain realtime scheduling. System limits, RTKit services and defaults were not
changed. The five-second PCM result is not a long-duration drift qualification.

Last completed job: `/var/tmp/tonenari-lab-drift-rt-diagnostic-v7`, exit 0 with
`owned_processes_reaped`. Its full guest evidence remains at
`/var/tmp/tonenari-drift-lab-Rhl56O`; those raw files were deliberately not fetched
after the stop instruction. Tool-observed completion and scheduling output are
preserved on the host as `drift-rt-v7-observed.txt`.

Restart only after the root operator resumes Linux testing with sufficient
memory and no competing VM measurement. The latest uploaded fixture is
`/var/tmp/tonenari-integration-drift-fixtures-v7-rt-diagnostic`; the tested native
snapshot remains v9, not the newer lifecycle-epoch implementation. A new run
must use a fresh unique job ID, never reuse or overwrite the existing job dirs.
For example, after explicitly choosing an appropriate latest frozen snapshot:

```sh
/Applications/UTM.app/Contents/MacOS/utmctl exec 77F2F097-29B8-4836-80F6-358AE38A2316 --cmd /usr/sbin/runuser -u state303 -- /bin/bash /var/tmp/tonenari-integration-20260907-clock-rate-v9/scripts/lab/guest-job.sh drift-resume-UNIQUE /usr/bin/env TONENARI_LAB_TRACE_ENABLED=true TONENARI_LAB_QUANTUM=128 /bin/bash /var/tmp/tonenari-integration-drift-fixtures-v7-rt-diagnostic/run-clock-drift.sh /var/tmp/tonenari-integration-20260907-clock-rate-v9 /var/tmp/tonenari-integration-drift-fixtures-v7-rt-diagnostic 2000 120 false
```

This exact example deliberately reproduces v9. To qualify current production,
substitute a newly frozen snapshot in both workspace arguments and verify its
hashes. Retain identical on/off conditions, record observed thread scheduling,
separate packet jitter from sample-rate ppm, and assess both native PCM and
RTP/clock-position evidence before making a capability claim. RTKit access or
system scheduling changes remain outside this private-only task's authority.

## Qualification limits

This is an installed PipeWire RTP-module DLL and ToneNari native-stream test.
It is distinct from ordinary fixed-rate stream conversion and from ALSA device
clock adaptation. PipeWire's
[clock property contract](https://docs.pipewire.org/devel/page_man_pipewire-props_7.html)
states that equal clock names bypass adaptive alignment; merely giving fixtures
different names is insufficient to establish independent clocks. No physical
ALSA driver, vendor clock, hardware unplug, other distribution or other OS is
qualified by these results. They must not alone enable an unconditional global
`driftCorrection` claim.

Evidence base:
`/Volumes/Linear 1/test/tonenari/evidence/native-device-parity-20260907/linux-integration`.
The initial production snapshot is
`/var/tmp/tonenari-integration-20260907-clock-rate-v5` in Ubuntu ARM64.
Its `TNPipeWire.c` SHA-256 is
`640f4e0cae06d55002ac3be98b77f68e9b319959d966e582ade8d7dc8242d349`.
The initial fixture archive `drift-fixtures-v2.tar.gz` has SHA-256
`9ba44675604d7c1412714bb965af8278d9388a7854e9725a71d53be2f5b3f379`.
