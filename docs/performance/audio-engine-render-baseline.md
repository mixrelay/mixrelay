# Audio engine render baseline

Date: 2026-09-03

## Allocation boundary

`RenderPlan::prepare` performs graph lookup, channel-route lowering, and exact
buffer-offset calculation on the control thread. `BlockProcessor::new` performs
the single render-arena allocation. `BlockProcessor::process_block` then uses
only the preallocated arena and caller-owned interleaved buffers; it contains no
allocator, lock, logger, graph traversal, or string lookup.

The unit fixture covering two sources, a Mixer, Gain, explicit channel swap,
virtual I/O, and a device sink reserves exactly 80 `f32` arena samples for a
four-frame block (320 bytes). The default macOS duplex pass-through reserves
512 arena samples for a 128-frame stereo block (2,048 bytes), plus two
caller-owned 256-sample transport blocks (2,048 bytes). Total steady worker PCM
storage is therefore 4,096 bytes, excluding fixed shared-ring capacity.

These are structural allocation counts and exact buffer sizes. A before/after
latency comparison is not available because the preceding implementation had no
installed end-to-end path and therefore produced no comparable non-zero capture
samples. The first executable contract schedules engine output 512 frames after
the matching render frame: 10.667 ms at 48 kHz.

## Installed Core Audio duplex baseline

The 2026-09-03 development run installed the Universal 2 HAL in Core Audio and
ran the engine service under the same `_coreaudiod` account as the plug-in host.
The smoke client generated samples from absolute output frame and channel,
waited through 2,048 warm-up frames, and compared captured input against the
same function at the declared 512-frame offset for three seconds.

The first run produced 280,576 exact, non-zero matches with zero invalid
callbacks. A same-condition second run then exposed a timeline restart defect:
only 30,712 of 281,600 samples matched and were non-zero (10.9%). Core Audio had
restarted sample time at zero while the shared render watermark still described
the preceding run.

The fix begins a new capture epoch and then a new render epoch when the first IO
client starts. It neither clears the ring nor reallocates it. After reinstalling
the same build configuration, three consecutive runs produced
280,576/280,576, 281,600/281,600, and 281,600/281,600 exact non-zero matches,
all with zero invalid callbacks. Repeated-start correctness therefore improved
from 10.9% to 100% under the same three-second workload. This proves the
declared pipeline offset, bit-exact pass-through, and stop/start
resynchronization for this workload; it is not yet a worst-case jitter, CPU, or
round-trip-latency benchmark.

## Dynamic graph activation baseline

The previous installed engine used one hard-coded `BlockProcessor`; changing a
graph required rebuilding and restarting the LaunchDaemon, so there is no
same-condition live-switch latency result to compare against. The replacement
prepares and validates the complete processor on the control thread. At an
audio block boundary, activation performs one non-blocking gate attempt, one
pending-pointer swap, one `Box` ownership transfer, one `mem::replace`, and
atomic revision updates. It performs zero allocations, deallocations, locks,
waits, graph traversals, or string lookups on the audio worker.

As alternative evidence for the previously unavailable live-switch case, the
installed 48 kHz/128-frame engine was switched without a service restart from
direct pass-through to a -6.0206003 dB Gain graph and back to direct. Each
three-second Core Audio phase compared 281,600 non-zero stereo samples:

- half gain: 281,600 / 281,600 matched within `1e-6`, zero invalid callbacks;
- restored direct: 281,600 / 281,600 matched, zero invalid callbacks;
- final engine status: active revision 5, submitted revision 5, zero reported
  underruns and overruns.

The comparison tolerance is required because Rust's dB-to-linear `powf`
conversion is not bit-identical to the C fixture's literal `0.5f`. This run
proves block-boundary activation and audible data continuity for this workload;
it does not yet claim a worst-case activation time.

The same use case was then repeated through the packaged Tauri UI rather than
`curl`. The UI applied Gain as revision 6 and restored direct routing as
revision 7. Both phases again matched 281,600 / 281,600 samples with zero
invalid callbacks; the final status reported zero underruns and overruns. This
also verifies the packaged app's IPC control transport independently of browser
CORS behavior.

## Variable-channel endpoint storage

The endpoint transport is allocated for the configured render and capture
channel counts instead of reserving 64 channels for every graph. With the same
16,384-frame capacity in both directions, `tn_endpoint_transport_required_bytes`
reports:

- 1 channel: 393,432 bytes (0.375 MiB, 95.45% below fixed 64-channel storage);
- 2 channels: 524,504 bytes (0.500 MiB, 93.94% below fixed 64-channel storage);
- 8 channels: 1,310,936 bytes (1.250 MiB, 84.85% below fixed 64-channel storage);
- 16 channels: 2,359,512 bytes (2.250 MiB, 72.73% below fixed 64-channel storage);
- 32 channels: 4,456,664 bytes (4.250 MiB, 48.48% below fixed 64-channel storage);
- 64 channels: 8,650,968 bytes (8.250 MiB).

The worker reserves two maximum-size 128-frame scratch blocks once at startup
(65,536 bytes total) so a channel-count switch performs no allocation in the
audio loop. Only the slices matching the active render and capture counts are
cleared and processed. This measurement covers deterministic storage size, not
resident-memory overhead or end-to-end CPU load.

## Reproduction

```sh
cargo test -p tonenari-audio-engine
cargo llvm-cov --workspace --lib --show-missing-lines \
  --fail-under-functions 100 --fail-under-lines 100 --fail-under-regions 100
pnpm install:macos-dev-audio
pnpm smoke:macos-installed-duplex
pnpm smoke:macos-dynamic-graph
```

The tests also fix the following behaviors:

- Gain and multi-input Mixer processing;
- automatic and explicit per-channel routes;
- `from_os -> to_os` routing inside one virtual endpoint;
- no output mutation on invalid block shapes;
- deterministic native shared-buffer silence for missing frames;
- arena-size overflow and malformed custom resolver rejection.
