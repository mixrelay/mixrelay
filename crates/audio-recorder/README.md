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
publish READY; Stop/Pause and failures publish the final valid partial only after
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
fail the recorder, never the audio graph. The RIFF 32-bit size boundary starts the
next numbered WAV rather than ending an otherwise healthy take.
Admission errors preserve and finalize the already accepted valid prefix; an
actual writer/converter failure stops that drain at its first failing packet.
An inactive writer blocks waiting for a command; only an open take uses the 2 ms
writer service interval. `stop_all` reports graceful-shutdown errors with node
identity and path before registry destruction.

Record creates an absolute destination with `create_new`, never overwriting an
existing file. Pause drains and finishes the current converter segment but keeps
the file open; Resume excludes paused time. The WAV rate and encoding are fixed
for a take. Each input packet retains its processing rate; a rate transition
finishes the old sinc-converter segment before preparing the next. Record and
Resume prepare the observed-rate converter before opening callback admission;
each packet's rate remains authoritative if the clock changes meanwhile. Fractional
output-frame carry prevents per-segment duration rounding from accumulating.
Same-rate float32 bypasses conversion and preserves sample bits. PCM16/24 apply
TPDF dither once, immediately before final integer quantization. Product sample
rate choices come from `tonenari_audio_io::formats::SAMPLE_RATES`.

## Automatic WAV splitting

A take automatically continues in `first-002.wav`, `first-003.wav`, and later
numbered files before a WAV would exceed RIFF's 32-bit size field. The first file
uses the requested path unchanged; later parts insert a minimum-three-digit
sequence before its extension (or use `.wav` if there was no extension). File
limits are rounded down to whole channel frames, reserving 256 bytes for headers
and padding. No frame is split between files.

`wave/files.rs` owns only the current writer, per-file frame count, ordinal and
file paths. It finalizes/closes the previous writer before exclusively creating
the next. Only one file writer is open; it does not retain an unbounded part list.
The original take path remains in `RecorderStatus.path` and is still used by
Resume. Status frames are cumulative across all parts. SRC history, fractional
output-frame carry and TPDF state belong to the take/signal writer and are never
reset by file rollover. No callback code or packet transport changes for splitting.

The next part is opened only when another complete output frame must be written.
Stop exactly at a file limit therefore produces no empty next part. A filename
collision, failed file creation, disk write, or WAV finalization fails the
recorder with the affected file path in its error. Existing files and previously
completed parts are preserved; the current failed part may be incomplete. Errors
are sticky so final cleanup cannot append duplicate PCM after a failed rollover.
Finalization is attempted even if SRC-tail processing returns an error.

The production rollover path is exercised with a small, internal `cfg(test)`
frame limit, not a public option and not a separate writer implementation. Tests
compare exact decoded PCM across parts at 1, 2 and 8 channels, verify PCM16/24
dither and float32 output against one unsplit file during input-rate transitions
and SRC-tail flush, preserve existing files on collisions, inject write/header
flush failures, and retain original-path/cumulative status through Pause/Resume.
No 4 GiB fixture was written: the same frame-boundary transition is tested with
small files, while limit arithmetic is checked against the real RIFF bound.
This is a functionality change, not a throughput optimization claim.

Final split-file checkpoint: 32 crate tests passed, zero failed/ignored (0.13 s);
strict Clippy with all four repository complexity gates, `cargo fmt --check`, and
`cargo doc --no-deps` succeeded. Independent review found no P1/P2/P3 in the
changed file-ownership, signal-state and failure boundaries. Installed-app and
long-duration disk validation remain separate integration evidence.

## Executed crate checkpoint

On 2026-09-09, these focused commands succeeded:

- `cargo test -p tonenari-audio-recorder -- --test-threads=1`: the initial
  21-test checkpoint passed with zero failed/ignored in 0.23 s. The subsequent
  packed-transport checkpoint is reported below. Doc tests: zero.
- `cargo clippy -p tonenari-audio-recorder --all-targets -- -D warnings
  -D clippy::cognitive_complexity -D clippy::too_many_arguments
  -D clippy::too_many_lines -D clippy::type_complexity`: exit 0.
- `cargo fmt -p tonenari-audio-recorder -- --check`: exit 0.

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
build policy: `tonenari-audio-recorder` now has `opt-level = 3`, like the existing
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
