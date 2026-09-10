# macOS native frame-credit pacing

## Implemented boundary

The prior macOS worker advanced its sample position by the configured DSP block
size on each wall-clock timer iteration. A matching nominal sample rate did not
make this a native clock. The worker now uses the following evidence order:

1. An in-use MixRelay HAL publishes actual WriteMix render credits or ReadInput
   capture credits. Active render wins over a later capture phase so the graph
   cannot consume render input before its producer supplies it. Capture-only
   streams remain independently paced by their actual ReadInput requests.
2. Without active HAL demand, a healthy hardware binding can supply cumulative
   native callback frames, actual rate and reconnect epoch.
3. Without a hardware master, an active application capture can supply actual
   captured-frame count/rate and transport epoch through the versioned registry.

Hardware/application credit is projected into graph frames using an integer
cumulative ratio. Fractional credit is retained; it is not rounded independently
on each callback. The selected master uses fixed-ratio conversion rather than a
PI controller that would compete with its own frame-credit pacing. Other native
bindings retain independent-clock SRC/drift correction. Master selection is
idempotent and changes are handed off to the owning conversion worker.

`ProcessCycle` carries real credited sample positions and explicit clock-domain
changes/restarts. It leaves `monotonic_ns` absent: no native timestamp was observed
at this boundary and none is invented. A 250 μs polling sleep only wakes the
worker; it cannot grant frames. Graph activation, pause/checkpoint, format changes,
meter processing, and stop handling remain reachable while credits are absent.

`observe_clock(Device, synchronized)` only reports synchronized when every
independent external binding supplies its own qualified bridge-ready evidence.
An actual idle HAL or explicitly idle application set uses `observe_clock_idle`;
the idle timer path can maintain UI meters but does not claim native pacing.
Missing evidence remains Unknown. Retained-window overruns mark a discontinuity
and retain overflow reporting instead of silently replaying old frame positions.

## HAL ABI compatibility

The optional clock record is appended after the version-1 transport's two audio
buffers. The header remains 72 bytes; its reserved word at byte 52 holds the
extension offset. Existing version, `total_bytes`, and render/capture offsets are
unchanged. An old peer ignores the reserved word and continues using the original
audio storage. A new peer checks the exact expected offset, actual mapped length
and extension magic before accessing the tail.

Fresh extended storage initially has generation zero: an old HAL has not supplied
clock evidence. New HAL bind/start/stop supplies lifecycle state and epochs;
nonempty native I/O callbacks publish direction-specific absolute frame credits.
Lifecycle replacement occurs at the existing quiescent HAL control boundary;
callback publication and snapshots use bounded lock-free atomic operations.
Epoch-tagged credits cannot satisfy a new stream timeline. No realtime heap
allocation, logging, waiting, or OS property query was added to the HAL callback.

## Verification

`cargo test -p mixrelay-engine-service --bin mixrelay-engine-service macos_pacing
-- --nocapture` covers:

- Unknown, explicit idle and native/graph rate mismatch without fabricated frames.
- Render-only and capture-only quantums 64, 96, 257, 511 and 128: all 1,056 frames
  processed in slices no larger than the configured maximum of 128.
- 375 repeated observations between callbacks: no additional native frames.
- Late attachment, epoch replacement and retained-window overrun boundaries.
- 44,100 native frames yielding exactly 48,000 graph-frame credits, including
  single-frame observations, reconnect and both rate changes.
- Application master identity/epoch changes and a rate-handshake fallback that
  avoids waiting forever before the consumer can request its new format.
- Capture-phase demand cannot run ahead of an active render producer's input.

Same-condition correctness comparison: after consuming one real 128-frame credit,
375 unchanged-credit observations advance **48,000 frames with the previous timer
rule versus 0 frames with the native pacer**. Both use 48 kHz / maximum 128-frame
settings. This is a deterministic frame-accounting fixture, not a realtime jitter,
latency, CPU or memory benchmark.

`scripts/check-macos-native.sh` passes ASan/UBSan and its isolated-library 100%
line/function coverage gates. Extended transport tests include an actual old-sized
allocation with no tail (ASan), short mappings, corrupt/unaligned/overflowing
offsets, corrupt magic, lifecycle/credit resets and forked cross-process clock
publication. HAL I/O tests check real read/write callback helpers; driver tests
exercise StartIO/StopIO. The universal HAL builds for arm64 and x86_64.

## Sample rate and buffer-size boundaries

The subsequent [buffer-property integration audit](2026-09-08-macos-buffer-format-sync.md)
found that CoreAudio BufferFrameSize is a client IO view, not a global virtual
device quantum. The attempted full-pair registration was removed after live
cross-process evidence. The DSP / native callback distinction below still applies.

The app-to-HAL sample-rate path uses the existing CoreAudio property request and
subsequent observed-rate confirmation; it does not assume a setter acknowledgement
means a completed HAL transition. OS-to-app rate following and transactional DSP
rebuild belong to the shared session/controller implementation. The pacer refuses
native HAL credits whose actual rate differs from the active graph rate.

The configured **Buffer size currently means maximum graph/DSP slice size**.
Actual HAL callback quantum is independent and can differ; the worker preserves
the remainder instead of truncating or demanding an exact-size callback. This
change does **not** negotiate `kAudioDevicePropertyBufferFrameSize`, change a user's
hardware buffers, or report a configured DSP maximum as a native accepted buffer
size. Native buffer negotiation needs an explicit request/observed-acceptance
contract, supported-size capabilities and ownership/rollback policy.

## Remaining validation / limits

No HAL was installed and neither the engine service nor coreaudiod was restarted
for this task. Therefore updated-HAL live render-only/capture-only/duplex pacing,
wall-clock jitter/latency and sustained device-dropout measurements remain pending
coordinated deployment. Hardware/application SRC worker tests and native fixtures
are owned by their respective integration tasks and are not replaced by these
frame-credit tests.

Old HALs remain audio-compatible but lack the optional clock capability. Their
Unknown fallback still uses the existing clock warning; this task does not add a
new wire/UI warning code distinguishing old-ABI capability absence from a detected
clock fault. No claim of global synchronization is made for an unqualified bridge.
