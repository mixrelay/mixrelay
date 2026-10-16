# Application source clock transport v4

This transport is private to the desktop process-tap pump and engine service.
Both processes use the current shared-buffer ABI and the required XPC
`source_transport_version: 4` contract. Missing or different versions are
rejected before an attachment or producer lease is granted. There is no raw-frame
or previous-protocol mode. `native/macos/common/source_control.rs` owns the codec.

`MRApplicationClock` follows the existing sample buffer, at its required byte
length rounded up to a 64-byte boundary. Both sides validate the complete mapped
length, alignment, magic, exact version/header length and lock-free atomics before
use. The trailer contains integers and atomics only, never local pointers.

The pump replaces NaN/Inf with zero before its stateful SRC and preserves every
finite input bit. Replacements reach the graph's `sampleFaults.sourceSamples`
through a fixed counter in the clock trailer, independently of PCM availability.
Producer claims validate the actual lease and epoch before publishing counts;
temporary contention retains them on the producer. Reset flushes the prior
current epoch's counts before advancing. One graph drain consumes each pending
count, and graph/status lifetime totals saturate.

Ownership:

- The engine consumer publishes requested graph rate/generation, its own next
  read position plus stream epoch, actual callback frame ticks, running flag and adaptive target backlog.
  A bounded sequence snapshot prevents torn position/tick/target observations.
  The initial target is rounded burst + excess publication drought + graph block,
  capped at half capacity. Unknown producer cadence bootstraps at 512 frames.
  Consecutive graph slices accumulate into measured consumer scheduling bursts.
  Starvation grows once per episode and holds the next unread cursor until the
  target is buffered. Live shrink and replay are disabled; only startup, epoch
  changes and unavoidable overwrite recovery anchor a cursor. Fixed atomic
  per-source counters are reported from the pump's off-realtime control step.
- The pump acknowledges the requested format only after resetting its SRC and
  shared timeline. It owns its write position, native rate/frame progress and
  native running flag. Native frame progress includes captured **and dropped**
  callback frames; amplitude is not a clock signal.
- An atomic nonblocking producer/control guard excludes reconnect from an
  in-flight retired-pump write/reset. The audio consumer never takes this guard.
  A monotonically increasing attachment lease is returned in the XPC reply along
  with that attachment's epoch, then validated against the mapped trailer. Old
  producers cannot write/reset after reconnect, including shared epoch wrap or
  two attachment replies arriving out of order.
- The engine selects the clock-master flag. That source uses a fixed nominal SRC
  ratio because the graph is paced by its native clock. Other sources use the
  independent-clock bridge with engine-consumed output FIFO feedback.

Conversion storage/filter allocation happens on the owned desktop pump worker,
not either audio callback. A successful pull's valid prefix alone is published;
its zero-filled unused tail is never treated as captured PCM. Native format
generation, graph rate, master selection and ring drops trigger a new timeline
and filter reset. Rate-only tap changes refresh through native control code;
unsupported layout changes are explicit errors requiring session recreation.

The consumer rejects unacknowledged rates and stale epochs.
`mr_engine_source_registry_clock` exposes actual native frame progression
for the engine pacer; the caller must detect lack of progression/stalls rather
than treating a stored running flag as proof of an advancing clock.

Verification uses the Cargo-owned `native_callback_and_hal_contracts` engine
test. It runs the current shared buffer, registry and producer reset/write helpers
under ASan/UBSan for the native fixture code, including
short/misaligned/old attachments, rate acknowledgement, read feedback, master
selection and retired-producer rejection. Desktop clock-pump tests cover rate
conversion and feedback authority. These fixtures do not establish live Core
Audio permission, sleep/wake or long-duration hardware drift qualification.

## Fixed-input conversion comparison (2026-09-07)

The pump fixture sends exactly 128,000 native frames in 1,000 equal 128-frame
blocks through the actual bridge, then drains every currently available output
prefix without flushing artificial input. The previous raw transport always
forwarded 128,000 frames regardless of the graph rate. The converter produced:

| Native → graph | Previous raw count | Converted count | Ideal rate-scaled count |
| --- | ---: | ---: | ---: |
| 44,100 → 48,000 | 128,000 | 139,317 | 139,319 |
| 96,000 → 44,100 | 128,000 | 58,799 | 58,800 |
| 48,000 → 48,000 | 128,000 | 127,998 | 128,000 |

The 1–2-frame difference is the unflushed filter tail, not padded valid PCM. This
is a rate-correctness comparison, not a CPU-speed or end-to-end latency benchmark.
Feedback fixtures also run 1,000 stationary observations (exactly zero correction),
then 1,000 actual engine-consumption observations: an overfull output FIFO produces
negative correction for an independent source, while the selected native master
retains zero correction.
