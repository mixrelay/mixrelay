# Linux FIFO over-read correction — 2026-09-06

Status: **one proven sample-loss bug corrected; live audio continuity still fails**.

## Cause and change

The Linux application-capture and virtual-render readers drained their nonblocking
FIFO until `WouldBlock`, even when the fixed PCM queue was full. A PipeWire quantum
larger than the queue therefore overwrote the beginning of the quantum. Subsequent
DSP calls exhausted the retained tail and substituted silence.

Both readers now share a byte-budgeted read bounded by available queue capacity.
Excess bytes stay in the kernel FIFO for subsequent blocks. This introduces no
allocation, no larger audio queue, and no unbounded drain loop. It does not replace
the independent timer with PipeWire's graph clock or guarantee upstream delivery.

The regression first failed against the unbounded reader, then passed with the
correction: 4,096-frame bursts through a 1,024-frame queue, using 128-frame DSP
blocks at 2, 16 and 64 channels. Another test checks fragmented, nonblocking reads
that stop within an interleaved frame. These are byte-preservation tests, not live
64-channel audio qualification.

## Controlled comparison

Same Ubuntu 24.04.4 ARM64 guest, PipeWire 1.0.5, 48 kHz, 128-frame engine blocks,
stereo direct graph, debug build and Node 24.19.0. Each trial starts and stops its
own transient user engine so queued samples from the previous trial cannot leak
into the next recording. Four guest VMs remained running on the same Mac host.
Three baseline trials followed by three corrected trials; not a randomized or
long-duration benchmark.

The probe writes a four-second 440/880 Hz signal (peak 0.125/0.0625), records the
explicit MixRelay virtual source, and checks signal continuity between first and
last active samples. Active fraction counts left-channel samples above 0.001;
even an ideal sine has near-zero samples, so 100% is not the expected baseline.
No host/default hardware device is changed.

| Trial | Before active fraction | After active fraction | Before longest gap | After longest gap |
| --- | ---: | ---: | ---: | ---: |
| 1 | 50.4426% | 99.4063% | 1,146 frames / 23.875 ms | 120 frames / 2.500 ms |
| 2 | 50.5782% | 99.3237% | 1,276 frames / 26.583 ms | 123 frames / 2.563 ms |
| 3 | 50.6458% | 99.4896% | 1,273 frames / 26.521 ms | 1 frame / 0.021 ms |
| Median | 50.5782% | 99.4063% | 26.521 ms | 2.500 ms |

Median active fraction improved by **48.8281 percentage points**. Median maximum
gap decreased by **90.57%**, but **2 of 3 corrected trials still fail** the unchanged
criterion (active fraction >97% and longest internal near-silent run <48 frames).
The prior non-isolated runs are excluded because engine reuse mixed queued audio
between trials. An attempt to run a root-owned, non-executable-for-user backup
binary failed preflight and is also excluded, not counted as an audio failure.

### Reproducibility

- Baseline engine SHA-256: `ea0b7a186f5cbdbc448339625d13c17cdf94e1d95776528bbaafe299a949989b`.
- Corrected engine SHA-256: `836a8b06215b51cf721d99d5775d25a317308dfbff0c1d75b28ea2dfae5c7242`.
- Identical probe SHA-256 for both groups: `d9a45a5d13ee6724a5a6f889fa72c2de3e48777c6792fa460823f2380d8f8d77`.
- Guest completion jobs: `ubuntu-fifo-isolated-before-20260906-1915` and
  `ubuntu-fifo-isolated-after-20260906-1920`; both aggregate exit codes are 1.
- Raw logs: `ubuntu-fifo-before-20260906.log` and `ubuntu-fifo-after-20260906.log`
  in the configured external lab evidence directory. Original failed baseline
  recordings' metric logs are retained; no PCM recording was persisted.
- Harness: `scripts/lab/guest-pipewire-check.sh`; pass absolute engine, Node and
  probe paths as a non-root logged-in Linux user. It refuses an occupied API port,
  stops only its successfully started unit, and runs all three measurements.

## Follow-up evidence and remaining work

A diagnostic-only probe revision added the position of the longest silent run.
Two further trials found 124/119-frame gaps approximately 0.856/0.555 seconds after
the first active sample. These are internal gaps, not just startup latency.
The transport's timer/graph-clock relationship and output FIFO behavior remain
suspects; these measurements do not establish which one caused the residual gaps.
Do not widen the passing threshold or report this backend as qualified.

Ubuntu workspace Rust library **and binary** tests: 60 passed. Debian engine binary
tests: 5 passed. Ubuntu engine Clippy (including tests, warnings denied): passed
after correcting a checked narrowing conversion in test data generation. Local
lab helper tests: 22 passed. `native-check.mjs` now includes `--bins`; earlier
122-test full-build totals excluded the engine binary's OS adapter tests.

Still unverified: native callback/clock-driven scheduling, negotiated rates and
quantums, drift handling, sample accuracy, end-to-end latency, live multichannel
routing, hardware/plugin editor behavior, sustained load and resource endurance.
Windows, macOS and other distributions are not qualified by this Ubuntu result.
