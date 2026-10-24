# Audio recorder ownership and validation

`RecorderRegistry` is a control-thread owner. It is keyed by the composed,
graph-local runtime node identity, not a displayed label. Preparing a replacement
render plan acquires another tap to the same session; discarding that plan does
not stop an existing take. A separate atomic tap count excludes the worker's and
registry's transport ownership from liveness. Reaping joins only sessions with
no taps. The existing writer loop also finalizes a take after its final tap is
retired, even without a UI/status client.

`RecorderTap::capture` observes the actual input rate even when stopped. When
recording, it publishes interleaved PCM into fixed recorder packet slots. Each
slot has atomic PCM words and a FREE/WRITING/READY/READING ownership state. Each
side attempts one state claim, never retrying or waiting for the other thread.
It does not allocate, free, lock, perform file I/O, wait, or wake a thread.
There is one admitted producer per session. Stop/Pause close admission while
retaining the admission token, wait for in-flight entrants, and preserve any late
producer error before draining. A previous token cannot enter a later take. Once
a failure has been drained and published to status, its old gate is acknowledged
so it cannot overwrite the next attempt's error.
The tap never owns a file or thread handle. Tap cloning and render-plan disposal
belong to control-side preparation/retirement.

Each session has 128 packets with capacity for 256 frames each. Small callbacks
append to a producer-owned partial packet. Full packets and rate transitions
publish READY; reaching the take's input-duration budget also publishes its final
partial packet and closes admission. Stop/Pause and failures publish a partial only after
closing admission and draining callback entrants. The worker never reads WRITING
PCM. At a stable rate, 32-frame callbacks can therefore use all 32,768 frames
(682.7 ms at 48 kHz), instead of consuming one slot each and exhausting at 4,096
frames (85.3 ms). Rate changes can produce shorter packets; this is not a universal
minimum time capacity. Only file recording is buffered; no monitor/output path
is delayed by this crate.
Preparation accepts the shared product limit of 1–8 channels; the UI default is
two. The registry permits at most sixteen sessions. PCM slot storage is at most
1 MiB per session, plus 128 fixed slot headers and one 8 KiB maximum writer
scratch packet, excluding converter and file buffers. Sixteen maximum-channel
sessions therefore reserve at most 16 MiB of atomic PCM and 128 KiB of writer
scratch, plus bounded slot/owner metadata. A callback accepts
at most 4,096 frames (at most seventeen slot claims including carried partial
PCM, with bounded sample stores). Pool
exhaustion, invalid input, non-finite PCM and write/conversion/finalization failures
fail the recorder, never the audio graph. Every take is a single RF64 file,
including small recordings; the 4 GiB RIFF boundary does not split a take.
Admission errors preserve and finalize the already accepted valid prefix; an
actual writer/converter failure stops that drain at its first failing packet.
An inactive writer blocks waiting for a command; only an open take uses the 2 ms
writer service interval. `stop_all` reports graceful-shutdown errors with node
identity and path before registry destruction.

Record creates an absolute destination with `create_new`, never overwriting an
existing file. Pause drains and finishes the current converter segment but keeps
the file open; Resume excludes paused time. The WAV rate and encoding are fixed
for a take. A duration limit is fixed at creation and survives Pause/Resume and
licence changes. Input frames consume an integer duration budget using the least
common multiple of supported rates. Reaching the budget publishes the final
partial packet; the worker drains it and the SRC tail without waiting for another
callback. The sink caps output at exactly the file rate times the allowed seconds.
Each input packet retains its processing rate; a rate transition
finishes the old sinc-converter segment before preparing the next. Record and
Resume prepare the observed-rate converter before opening callback admission;
each packet's rate remains authoritative if the clock changes meanwhile. Fractional
output-frame carry prevents per-segment duration rounding from accumulating.
Same-rate float32 bypasses conversion and preserves sample bits. PCM16/24 apply
TPDF dither once, immediately before final integer quantization. Product sample
rate choices come from `mixrelay_audio_io::formats::SAMPLE_RATES`.

## One RF64 file per take

All recordings use RF64 from creation, with the `.wav` extension. There is no
RIFF compatibility mode, format-policy selector or numbered-file rollover. Small
recordings use the same container as large ones. External readers must support
RF64; legacy RIFF-only readers are not supported for new recordings.

`wave/files.rs` exclusively creates one buffered file and retains sticky I/O
errors. `wave/files/container.rs` owns the RF64 container and its 64-bit byte
counter. It reuses Hound only for the validated `fmt` header and sample encoding,
so the existing channel layout, PCM16/24 quantization and float32 sample bytes are
preserved without Hound's 32-bit data-size counter. `ds64` is the first chunk;
it holds 64-bit RIFF size, data size and per-channel frame count. RIFF/data sizes
use the RF64 sentinel. Float files include a `fact` sentinel referring to the
`ds64` count. Odd-length PCM24 data receives an alignment byte excluded from its
data size and frame count. Finalization updates sizes and flushes before a
successful Stop is returned. Creation, writing, seeking or flushing errors
include the recording path and fail the recorder without affecting routing.
Finalization is attempted even if SRC-tail processing fails.

Pause/Resume retains the file and original path. Status frames count the take's
actual output frames. Resampling, fractional frame carry, dither, packet storage
and callback behavior retain their existing ownership. No file I/O was moved to
the audio callback.

Validation covers empty and short files in all three encodings at every supported
channel count (1–8), exact PCM, alignment, pause/resume, rate transitions, packet
chunking,
accepted-prefix retention and injected write/seek/flush errors. Sparse files
cross the real 4 GiB boundary in PCM16, PCM24 and multichannel float32: a sparse
zero prefix is followed by real written PCM, with 64-bit sizes and frame counts
checked after finalization. This validates addressing, not sustained throughput.

Hermetic decoder checks validate RF64 sizes with the in-repo reader and decode
PCM with Hound. Because Hound reads RIFF, the tests translate container sizes
while retaining the production format headers and sample bytes. Sparse fixtures
cover complete frames immediately below, at (where frame alignment permits), and
above 4 GiB for every encoding/channel count. Their real tail PCM is read beyond
the boundary and decoded through a bounded RIFF view. This checks addressing and
format compatibility without reading gigabytes of zeros or using external tools.
Control-owner disconnection drains and finalizes the accepted prefix in every
format/channel count. Abrupt termination of the writer process or power loss
cannot execute this finalization and is outside these checks.

The container follows [EBU Tech 3306](https://tech.ebu.ch/docs/tech/tech3306v1_1.pdf).

The 2026-10-10 checkpoint passed 33 recorder unit tests and both explicit FFmpeg
tests, 119 audio-engine unit tests, nine test-support tests and five engine-service
recorder lifecycle tests. Recorder/test-support Clippy passed with all four
complexity gates; audio-engine Clippy passed its normal repository warning gates.
Formatting and diff whitespace checks passed. The checkpoints below are historical.


## Executed crate checkpoint

On 2026-09-09, these focused commands succeeded:

- `cargo test -p mixrelay-audio-recorder -- --test-threads=1`: the initial
  21-test checkpoint passed with zero failed/ignored in 0.23 s. The subsequent
  packed-transport checkpoint is reported below. Doc tests: zero.
- `cargo clippy -p mixrelay-audio-recorder --all-targets -- -D warnings
  -D clippy::cognitive_complexity -D clippy::too_many_arguments
  -D clippy::too_many_lines -D clippy::type_complexity`: exit 0.
- `cargo fmt -p mixrelay-audio-recorder -- --check`: exit 0.

Regressions cover exact interleaved float PCM, Pause/Resume/Stop, same-identity
reprepare, discarded preparation, final-tap retirement, more than sixteen retired
identities without polling, Stop racing capture, an admitted producer failing
after cutoff, fixed-rate conversion across input-rate changes, all 36 supported
rate pairs with a one-frame tail, integer clipping/dither, no overwrite, queue
overrun with accepted-prefix retention, error ownership across attempts, graceful
shutdown with live taps, failed-take error retention across channel replacement,
RIFF bounds, and injected disk-write/header-flush failures.
Additional packet regressions hold a worker slot in READING throughout capture,
hold an unpublished WRITING slot throughout a consumer attempt, verify PCM/rate
FIFO over 4,096 concurrent packets and ring wraps, and clone/drop taps on eight
threads without changing packet ownership. Filesystem
fixtures use the checkout-owned test support directory, not `/tmp`.

The callback allocation counter reports zero allocations and zero net allocation
count for normal capture, full-queue rejection and a preempted worker slot. The
implemented transport has no callback-side retry loop or worker-progress wait.
Control-side cutoff intentionally waits for admitted entrants before draining.

## Serial before/after capture-cost probe

The unchanged `examples/capture_cost.rs` was executed as a release binary on the
same host, without custom `RUSTFLAGS` or concurrent project builds/tests. Each of
three trials recorded 4,096 callbacks of 256 stereo float32 frames at 48 kHz,
with 300 microseconds of pacing outside the measured capture call. Each finalized
exactly 1,048,576 frames. The baseline was the earlier ArrayQueue transport;
it was measured before replacement and was then removed, not retained as a path.
Two earlier preparation runs overlapped project activity and are excluded.

| Transport / trial | Mean ns | Median ns | p95 ns | Maximum ns |
| --- | ---: | ---: | ---: | ---: |
| Before / 1 | 506 | 375 | 1125 | 17000 |
| Before / 2 | 908 | 875 | 1667 | 28667 |
| Before / 3 | 940 | 834 | 1625 | 85750 |
| After / 1 | 470 | 375 | 709 | 32834 |
| After / 2 | 549 | 417 | 834 | 37542 |
| After / 3 | 488 | 416 | 750 | 23667 |

The equally weighted mean of the three trial means was 784.7 ns before and
502.3 ns after (approximately 36.0% lower in this sample). Host scheduling and
timer overhead remain uncontrolled; the short probe is not proof of a universal
speedup or a worst-case audio deadline. The correctness improvement is the
one-attempt slot protocol, independently exercised with a preempted peer.

These tests and measurements do not establish installed-app
recording, native device behavior, UI transitions, sustained disk throughput, or
end-to-end engine/API integration. Those remain the integrating task's checks.

## Small-quantum debug recording investigation

The integrating native run reported an intermittent queue overrun after 348,672
output frames while converting 48 kHz / 32-frame stereo input to 96 kHz PCM24.
Its next take completed more than 92 seconds. The successful-run profile shows
the writer both waiting in `recv_timeout` and doing generic SRC work; it does not
identify the scheduling event responsible for the failed take. No QoS change was
made without evidence of a QoS defect, and the failure is not suppressed.

`examples/paced_recording.rs` reproduces the geometry with 15,000 absolutely paced
32-frame callbacks at 1,500 calls/s. It records ten seconds at 48 kHz to 96 kHz
PCM24 and asserts exactly 960,000 finalized frames. It accepts output rate,
encoding, and channels as arguments. Fixtures remain checkout-owned. Measurements
use `/usr/bin/time -l target/debug/examples/paced_recording 96000 pcm24 2`,
without concurrent project builds/tests. One intermediate run overlapping an
external media inspection was excluded. Timer pacing and host scheduling are not
a CoreAudio clock or worst-case deadline proof.

Changes were measured in two distinct stages. Stage 1 explicitly changes the dev
build policy: `mixrelay-audio-recorder` now has `opt-level = 3`, like the existing
audio DSP package overrides. Optimizing the rubato dependency alone left generic
SRC instantiated in the recorder under the caller's unoptimized dev policy.
Stage 1 also prepares the first/resumed converter before opening admission. Its
2-channel Record preparation moved from 126–138 µs to 856–889 µs in the sampled
runs; that work previously happened after callback admission. The successful
paced baseline does not prove this was the cause of the intermittent native loss.

| Debug stage | CPU user + system seconds, three 10-second stereo takes | Mean seconds |
| --- | --- | ---: |
| Original recorder dev opt0, packet per callback | 1.68 / 2.42 / 1.50 | 1.867 |
| Stage 1: explicit dev opt3 + converter preparation, same packet transport | 0.89 / 0.91 / 1.03 | 0.943 |
| Stage 2: same dev opt3/preparation, partial-packet packing | 0.99 / 0.96 / 0.86 | 0.937 |

Stage 1 reduced sampled whole-process CPU time by approximately 49.5%; this is
not attributed to packet packing. Every listed take finalized all 960,000 frames.
The isolated eight-channel probe also completed: 2.55 CPU seconds originally,
2.01 seconds at stage 1, and 1.70 seconds at stage 2 (one run each, not a stable
percentile comparison).
Packing's approximately 0.7% mean CPU difference is within the observed scheduling
variation and is not claimed as a speedup. Its improvement is using eight times
as many actual frames in the same PCM allocation for stable 32-frame callbacks,
with no change to the worker's 2 ms service interval or callback waiting policy.

The packed-transport regressions pass 28 tests with zero failed/ignored (0.12 s),
strict Clippy including all four repository complexity gates, and formatting.
They include 500 ms of 32-frame input while deliberately never servicing the
writer, then actual 8-channel 96 kHz PCM24 finalization at exactly 48,000 output
frames and every decoded PCM sample nonzero. The test uses the same fixed
storage, no callback allocations, and no
wall-clock timing assertion. Rate changes, partial-packet cutoff, admitted errors,
FIFO wraps, and a carried partial plus the maximum callback are also exercised.
This demonstrates fixed-frame capacity utilization, not recovery from unlimited
writer starvation or a confirmed fix of the original installed-engine failure.
