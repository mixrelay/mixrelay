# macOS hardware native-rate SRC and recovery

## Implementation and ownership

Hardware input/output keep the device's observed native Float32 stream rate. A
48 kHz graph no longer requires the native device to run at 48 kHz. Native format
validation rejects mixed rates, malformed layouts, unsupported sample encodings
and nonintegral/out-of-range rates instead of reinterpreting PCM. No hardware
rate setter is used by this path.

There are two bounded 16,384-frame SPSC queues per binding: native callback to
conversion worker, and conversion worker to graph (reversed for output). The
worker alone owns the mutable `AsyncClockBridge` and preallocated scratch. The
shared `Arc<Device>` never exposes a mutable converter. Native callbacks use
bounded copying and atomics, without new allocation, mutexes or property queries.
Only a queue's own consumer discards its cursor at an epoch boundary; output
flush requests are acknowledged by the native callback, not an opposite-owner
cursor write. Converter priming zeros are not enqueued as captured audio.

Independent bindings use sinc conversion and bounded drift correction. Feedback
uses real cumulative frame progress, not an elapsed polling interval or signal
amplitude. Render queue feedback includes the converter's internally buffered
source-equivalent frames. The selected hardware graph-clock master uses fixed
conversion rather than a controller fighting its own clock. Its stable owner ID
survives bridge priming/reconnect epochs; missing or failed owners are not clock
evidence. Per-binding synchronization remains false until valid bridge output
has actually been produced. Main frame-credit pacing is documented separately in
`2026-09-07-macos-native-pacing.md`.

Session format changes preallocate converters before any native/controller
commit. Cancellation leaves active owners untouched. Commit publishes the pending
converter and new graph rate under one mailbox lock; activation and queue reset
belong to the worker. Until its ready-rate acknowledgement matches the graph,
the graph cannot consume or enqueue old-rate PCM. Runtime capability and external
format operations delegate through the macOS wrapper instead of disappearing at
that abstraction boundary.

## Reproducible checks

```sh
bash scripts/check-macos-device-io.sh
bash scripts/check-macos-device-io.sh --live
cargo test -p tonenari-engine-service --bin tonenari-engine-service macos_devices::tests
cargo clippy -p tonenari-engine-service --all-targets -- -D warnings
```

The live workflow requires the installed ToneNari virtual endpoint, creates only
unique owned temporary aggregates and runs both ignored `native_coreaudio_`
tests serially. It does not select defaults, change existing device rates, edit a
saved graph, restart services or capture a real microphone. Native output PCM is
silent. Test fixture processes and aggregates must be gone after completion.

The deterministic C ASan/UBSan suite passed 384 callback/layout/epoch cases plus
64 three-owner SPSC/epoch cases, teardown faults, default-selector changes,
mixed-stream channel subsets and actual native-rate validation. Six focused Rust
tests passed, including 1/2/16/64-channel worker transfers, no fabricated priming
output, staged-format cancellation, stale-rate silence, stable master identity
and concurrent converter/rate publication.

Native C recovery passed four disconnect/reconnect cycles. The standalone Rust
recovery test passed four cycles while retaining graph owners and offline gain
edits. The native SRC test retained native input/output at 48,000 Hz and the same
device owners through graph 44,100 -> 96,000 -> 48,000 Hz, measuring two seconds
per rate. Actual native cumulative frame credits drive graph processing; sleeping
for a nominal graph block duration is deliberately not used as a clock.

## Development-build throughput regression and correction

Both compared runs used the same temporary-aggregate fixture, native 48 kHz,
256-frame graph slices, 16,384-frame queues, silent PCM, two-second measurement,
and native frame-credit pacing. The ordinary unoptimized development DSP failed
at graph 96 kHz. Optimizing only `tonenari-audio-io`, `rubato` and
`audioadapter-buffers` at level 3 removed the observed queue losses. These package
settings are now included in the workspace development profile; the remaining
development code retains debugging configuration.

| Graph 96 kHz measurement | Before: unoptimized DSP | After: optimized DSP |
| --- | ---: | ---: |
| Actual native frames | 96,256 | 96,256 |
| Valid captured graph frames | 81,152 | 191,488 |
| Submitted graph frames | 189,696 | 192,512 |
| Input FIFO overflow events | 37 | 0 |
| Output FIFO overflow events | 11 | 0 |

Valid capture throughput increased approximately 136% under that matched native
frame count. This is a short development-build processing-capacity check, not a
release CPU, acoustic latency, pitch accuracy or long-duration benchmark. The
first diagnostic fixture used imprecise block sleeps and was rejected as clock
evidence; its observations and all subsequent failed attempts remain preserved.

The comparison command was the SRC test below with three additional Cargo
`--config 'profile.dev.package.<name>.opt-level=3'` arguments for the packages
listed above. The final default-profile rerun used no overrides:

```sh
TONENARI_MAC_DEVICE_FIXTURE=/tmp/tonenari-hardware-src.x07O8G/live \
  cargo test -p tonenari-engine-service --bin tonenari-engine-service \
  native_coreaudio_src_uses_actual_rate_and_rebuilds_graph_format_only \
  -- --ignored --nocapture --test-threads=1
```

It passed all three graph rates, with input/output overflow counts 0/0 and no
unexpected epoch change. At 96 kHz it observed 95,744 native frames, 190,464 valid
captured frames and 191,488 submitted frames. Native rates remained 48,000 Hz.

Evidence, including unsuccessful runs, is retained at
`/Volumes/Linear 1/test/tonenari/evidence/native-device-parity-20260907/macos-hardware-src/`.
The initial combined live-workflow rerun passed native C recovery but stopped at
an unrelated new application-rate fixture's missing trait method before Rust
tests ran; `live-workflow-v1-build-failed.log` records that failure separately.

The next combined attempt exposed an aggregate publication race: creation had
acknowledged before both native stream layouts existed. The fixture now waits
for observed input/output format readiness before its UID handshake, without
retrying or suppressing graph preparation failures. `live-workflow-v2-fixture-not-ready.log`
preserves that attempt. Final `live-workflow-v4-final-pass.log` records the full
default-profile workflow: sanitizer checks, four C recovery cycles and both Rust
native tests passed (2 passed, 0 failed; 11.69 seconds for the Rust tests).
Strict engine-service all-targets Clippy also passed after the final wrapper
delegation changes.

Two orphan probe UIDs from earlier failed attempts were found by independent
inventory, despite their creator processes having exited. Their precise creation
attempt cannot be reconstructed because early fixtures lacked per-UID logs. Root
confirmed this was the sole live runner. After exact UID, dead creator, aggregate
class, sole ToneNari subdevice and non-default checks, normal CoreAudio destruction
removed only these two probes; `owned-orphan-cleanup.log` records the action.
Final fixture teardown now logs each created/removed UID and checks its actual
disappearance, failing instead of trusting destruction acknowledgement alone.

The final independent inventory is byte-for-byte identical to the initial
inventory: eight original device UIDs with unchanged rates and default
input/output IDs 623/690. Final fixture PIDs 24221, 24277 and 24337 are absent.
This task did not stop an application or restart an audio service. The observed
desktop process changed independently during concurrent root work, so general
process-list equality is not claimed.

## Remaining limits

The live fixture is a CoreAudio aggregate backed by ToneNari's virtual HAL, not
physical multichannel hardware qualification. It proves actual native callbacks,
owner retention, selected-rate conversion and recovery, but not hours of drift
between independent physical clocks or nonzero live acoustic content. Full
64-channel realtime capacity remains unmeasured; deterministic channel-boundary
tests are not a substitute. Permanently hung native stop/unregister calls retain
the existing bounded-return worker quarantine policy; an absolute leak-free
claim for an unresponsive third-party driver is not made.
