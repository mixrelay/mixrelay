# Initial physical-input admission — 2026-09-14

## Cause and bounded correction

The single startup underrun retained in `2026-09-14-node-render-scaling.md` was
reproduced. `graph_read` discards queued PCM on its first consumer-epoch admission
so samples left from device/format preparation cannot enter the new graph timeline.
The engine previously reported that intentional empty first transfer as `Active`,
which counted it as an input shortage. Native steady-state starvation counters
remained zero. This is different from a shortage after playback has started.

The native reader now owns `Option<u64>` epoch state and returns a small `GraphRead`
result containing actual frames and a one-time `initial_epoch` flag. The engine
classifies only an incomplete transfer with that flag as `InputActivity::Priming`.
It still reports zero actual frames, clears stale PCM and never publishes synthetic
priming as valid samples. A second empty read, a later starvation, or a subsequent
recovery epoch remains Active and is not exempted. Invalid/zero-sized requests do
not consume the first-admission flag. No timer-based grace window or counter reset
was introduced.

Only the serialized graph consumer accesses this epoch state; callback/control
ownership and queue cursor rules are unchanged. The existing FFI return contract,
Rust caller and C header were updated together (8-byte result); no alternate API or
old return contract remains. No HAL install or system audio restart was needed.
No buffer capacity, queue target, sample rate, DSP operation or scheduling interval
changed, so this is an accounting/state correction, not a claimed latency or CPU
speedup. The current 32-frame/48-kHz block represents about 0.667 ms of startup time.

## Same-route installed measurements

Normal app quit/start was used, with desktop and its server verified stopped before
each launch. The existing mono 96-kHz microphone -> 48-kHz/32-frame graph -> stereo
virtual bus / speaker and second-graph 96-kHz headphones route was preserved.
No graph, gain, rate or OS-default writes were made. Startup restore advances the
first graph's revision; its authored topology and controls were not edited.

Each row is 20 read-only status samples, 500 ms apart after API availability. Values
are cumulative since that new engine process started, not reset at sampling time.

| Build / launch | UTC observation window | Underruns | Overruns | Discontinuities |
| --- | --- | ---: | ---: | ---: |
| Before | 04:10:40.155–04:10:49.770 | 1 -> 1 | 0 -> 0 | 0 -> 0 |
| After 1 | 04:12:18.468–04:12:28.079 | 0 -> 0 | 0 -> 0 | 0 -> 0 |
| After 2 | 04:13:22.400–04:13:32.020 | 0 -> 0 | 0 -> 0 | 0 -> 0 |

Every sampled session was running with issue null, 48,000 Hz and 32 frames. Before
engine SHA-256: `12ec172df5d3494d22d819189ff039326374cb6fce064e1e4171efa0d463289f`.
After normal engine SHA-256:
`69ec176d748c3c87a9671b7096f7aecca361d2cc3aaaadd2bba01b4b7dacd805`.
The second after launch uses PID 35525; it is left running.

## Verification

The regression first failed with `Active` instead of `Priming`, then passed. It
checks startup discard, immediate subsequent shortage, fresh PCM, a later epoch,
and zero allocations in the initial read path. Native FFI tests cover rejected
requests, epoch zero, first admission, actual PCM, retained native starvation
counting and later recovery discard.

Focused serial results: 27 native device-I/O tests, 1 startup regression, 3 activity
tests and 5 callback-owner tests passed. Strict Clippy for all engine-service and
macOS transport targets, engine build, documentation generation, workspace fmt,
C-header syntax and diff whitespace checks passed. No whole-workspace or
cross-platform pass is claimed. Code and ownership self-review was performed by
the primary agent; no independent agent was used.

Final installed capture **passed**: 30,000 ms, graph revision 752 unchanged,
1,439,744 frames per channel and 2,812 native callbacks. Both channels contain
only finite nonzero samples, with zero malformed buffers, exact-zero runs or
native callback timestamp gaps. Counters stayed at zero from before capture open
through after stop. The two WAV analyses also found zero exact-zero frames and
continuous file timestamps. Local prefix:
`output/microdropouts-20260914-1105/startup-admission-final`.

The capture is the existing OS-visible virtual bus, not headphone DAC loopback or
an independent reference for every possible nonzero click. PCM sampling starts
after the recorder's 500-ms settling period; the counters cover capture admission
as well. The normal engine build was restored after the integration-test relink,
matching the launch SHA above; the tested process was not restarted afterward.

The earlier rare native microphone clock-gap root cause is not addressed by this
narrowly scoped startup correction. No commit or push was performed.
