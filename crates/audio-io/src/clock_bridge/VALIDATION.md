# Shared clock boundary: component qualification

2026-09-07, local macOS ARM64. This verifies the shared implementation, not an
OS adapter, physical device, daemon scheduling deadline, or global capability.

## Implementation and ownership

- `rubato` 5.0.0 async sinc, default 256-tap Blackman-Harris window, cubic sinc
  interpolation and 128x filter oversampling. Default FFT/log features disabled.
  License: MIT OR Apache-2.0. `audioadapter-buffers` 5.2.0 interleaved views.
- Official implementation references: [rubato real-time/async guidance](https://docs.rs/rubato/5.0.0/rubato/),
  [Async](https://docs.rs/rubato/5.0.0/rubato/struct.Async.html).
- Construction, scratch/ring allocations and destruction belong off the audio
  thread. One mutable worker owns conversion and feedback. This is not a
  cross-thread transport. Adapters supply their own bounded SPSC FIFOs.
- Matching nominal Hz does not prove a common clock. `Independent` always uses
  sinc, even at 1:1. `ProvenSameClock` uses fixed-ratio sinc for different rates
  and exact copy only for equal rates.
- Exactly one configured feedback FIFO controls a boundary. Input/output queue
  occupancy, target and actual advanced-frame count use that queue's rate.
  Capture workers observe the downstream graph FIFO, not greedily drained local
  input. Render workers add `buffered_input_frames()` to external source backlog.
  High occupancy reduces output/input ratio. PCM level never drives feedback.
- PI correction is bounded by caller-configured 1..2000 ppm, with conditional
  integral anti-windup and a 200 ppm/second slew bound. Stopped/unknown clocks
  freeze the integrator; explicit epoch reset clears it and the sinc tail.
- Pull reports real consumed input and transferred output, queued frames,
  algorithmic output-frame delay and epoch. Zero-filled shortage is not valid
  transferred PCM. Adapters must enqueue only the valid prefix.
- Short quantum starvation retains sinc history. Actual dropped packets,
  restart, format/timeline changes require explicit reset plus external FIFO
  flush. New rate/channel configurations require off-thread reconstruction.
- Memory is bounded, not universally small: maximum permitted 262144 source
  frames at 64 channels consumes 64 MiB for the input FIFO alone. The numerical
  fixture uses 8192 source frames (2 MiB at 64 channels), not the maximum.

## Reproduction

```sh
cargo test -p mixrelay-audio-io clock_bridge --release -- --nocapture
cargo clippy -p mixrelay-audio-io --lib --tests -- -D warnings
```

Long simulated clocks are deliberately tested in release mode: unoptimized sinc
arithmetic makes the same deterministic simulation take minutes without adding
coverage. The tests do not sleep for simulated time.

## Equal-condition before/after drift measurement

Both paths use identical sinc, 48 kHz nominal rates, mono silent PCM, 128-frame
producer packets, a 1024-frame initial downstream occupancy, and 180 simulated
seconds. The baseline leaves correction disabled. Its value below is cumulative
produced-minus-consumed balance; a real bounded FIFO would underrun/overflow
before reaching a negative/excessive balance. The corrected path checks actual
occupancy is always inside 0..4096 frames. Silence deliberately proves signal
level is irrelevant to clock tracking.

| Producer offset | Baseline net frame drift | Corrected occupancy min..max | Final occupancy | Final correction |
| --- | ---: | --- | ---: | ---: |
| -500 ppm | -4322 | 935..1025 | 1024 | +500.345 ppm |
| -100 ppm | -864 | 1006..1025 | 1024 | +100.029 ppm |
| +100 ppm | +864 | 1023..1042 | 1024 | -100.009 ppm |
| +500 ppm | +4318 | 1023..1113 | 1024 | -499.846 ppm |

No CPU performance improvement is claimed: the prior missing boundary had no
equivalent correct SRC implementation. The comparison measures elimination of
frame-balance drift under identical audio/clock conditions, not speedup.

With heterogeneous 37/256/73/511/128/19-frame native quantums and real conversion,
180-second +500 ppm 44.1→48 kHz occupancy was 1926..2254 (target 2048, final 2024),
and -500 ppm 48→44.1 kHz was 1910..2128 (final 2027). Both stayed bounded.

## PCM, lifecycle and memory evidence

- 997 Hz, 0.5-amplitude tone, 44.1↔48 kHz, 1/2/8/16/32/64 channels, arbitrary
  input quantums: measured amplitude 0.499916..0.499991; RMS 0.353523..0.353550.
  All inactive channels remain exactly zero (no cross-channel contamination).
- 30 kHz tone downsampled 96→44.1 kHz: residual RMS 0.000000032 versus input
  RMS 0.353553391. This checks anti-alias rejection, not only frequency mapping.
- 8↔384 kHz at 1/64 channels checks extreme supported geometry and bounded
  output with no fabricated transferred frames.
- Thread-local allocator instrumentation counts allocation, zeroed allocation,
  reallocation and deallocation. For each 1/2/8/16/32/64-channel case: 100
  push/pull/ratio-feedback cycles plus five resets performed **zero heap operations**.
  This is an owned component-path assertion, not an OS process leak guarantee.
- Saturation for 100 seconds stays within ±1000 ppm and returns to zero within
  five seconds when queue error disappears, without retained integral windup.
- Reset after accumulated correction is sample-for-sample equal to a freshly
  constructed converter, proving queued audio, filter tail and ratio reset.
- Invalid shape/nonfinite input, over-capacity packets, wrong epoch/feedback
  authority and invalid frame durations are rejected. Priming is distinct from
  running underflow, and stopped clocks do not accumulate correction.

Native deadline/latency, hardware sample-clock changes, long physical drift and
adapter FIFO-discontinuity handling remain adapter-level acceptance work.
