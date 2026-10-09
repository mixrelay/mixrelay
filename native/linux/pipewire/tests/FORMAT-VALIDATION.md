# Linux native format contract and isolated tests

The session sample rate is the MixRelay-owned stream's negotiated F32 PCM rate.
The PipeWire server graph clock is independent of that setting. Its adapters
perform sample-rate conversion; `GraphTimeline` / `FrameBudgetMapper` convert
callback duration to render frame units, not PCM. Adding another PCM converter
at this boundary would resample audio twice.

The UI processing buffer is the maximum MixRelay DSP block (`CycleBlocks`).
`node.latency` requests a scheduling preference but does not set the server's
quantum or another application's buffer. No global metadata is changed. Actual
variable callback quantum is an internal observation, never an
`observedDeviceBufferFrames` value or a reason to overwrite the selected buffer.

The private daemon follows PipeWire's VM policy: a 1,024-frame native minimum
inside a VM, with 128-frame MixRelay DSP blocks. The normal Debian 13 / PipeWire
1.4.2 forward-and-mute test retained 480,000 frames with no missing, repeated or
invalid samples, output read shortage or clock discontinuity during measurement.
Forcing native 128-frame scheduling had lost source frames before engine ingress.
This is a lab configuration correction, not a production performance comparison
or proof of lossless 128-frame scheduling on physical Linux hardware.

Actual Ubuntu ARM64 / PipeWire 1.0.5 isolated-daemon measurements (clock-rate-v10):

| Selected PCM rate | Processing block maximum | Native graph quantum | Observed stream PCM callback |
| --- | --- | --- | --- |
| 44,100 | 256 | 128 | 117 or 118 |
| 48,000 | 1,024 | 128 | 128 |
| 96,000 | 128 | 128 | 256 |
| 44,100 | 64 | 128 | 117 or 118 |

Each transition activated matching native owners and DSP revision and observed
real application PCM through the new revision. Source/graph combinations
44.1/44.1, 48/44.1, 44.1/48 and 48/48 produced 48 kHz PCM with a 999–1,000 Hz
zero-crossing estimate of the generated 1 kHz tone over 48,000 retained samples
after startup discard. This is a frequency estimate, not precision measurement.
Matching PipeWire graph clock IDs does **not** prove independent hardware-clock
drift correction; the separate RTP fixture qualifies its own network-clock scope.

Run only in a uniquely named `<checkout>/.lab/mixrelay-integration-*` source snapshot
inside the lab guest. The harness creates a private runtime, config, state and
D-Bus, disables hardware monitors, then reaps only its owned processes. It does
not alter services, default devices, system configuration, or user graphs.

```sh
MIXRELAY_LAB_GRAPH_RATE=44100 MIXRELAY_LAB_SOURCE_RATE=48000 \
  MIXRELAY_LAB_TEST_NAME=native_pipewire_rate_conversion_and_observed_clock \
  bash native/linux/pipewire/tests/run-application-retracking.sh "$snapshot"
MIXRELAY_LAB_TEST_NAME=native_pipewire_live_session_format_transaction \
  bash native/linux/pipewire/tests/run-application-retracking.sh "$snapshot"
MIXRELAY_LAB_LIFECYCLE_TEST=1 \
  bash native/linux/pipewire/tests/run-application-retracking.sh "$snapshot"
```

The lifecycle test uses only its own source and observer. It deliberately queues
obsolete PCM around native state transitions to check the production epoch
handoff. Keep its result separate from ordinary ring unit tests; an unexecuted or
failing native fixture is not lifecycle acceptance.

The output callback sets `pw_buffer.size` to the emitted audio frame count,
including silence, and clears it before reusing an output buffer. PipeWire uses
this field for queued-frame time accounting; `spa_chunk.size` remains bytes.
The following C regression test exercises the production callback with the real
PCM ring and mocked dequeue/queue calls. It covers all four PCM formats, buffer
capacity, silence, invalid format/geometry, and returning each buffer once.
It starts no daemon and accesses no audio devices:

```sh
bash native/linux/pipewire/tests/run-stream-buffer-metadata.sh
```

Correct frame accounting does not make the realtime graph lossless when a
consumer misses its processing deadline. The controlled reader-delay failure
and source-policy experiments are recorded in `docs/product.md`.

Startup preparation touches every native PCM ring page and the Linux driver's
callback scratch pages on control before publishing them. The loopback processor
also waits for the capture/output format after preparing the render/input
dependency, before DSP publication. This adds no fixed sleep to RT processing.
The existing two-quantum FIFO prefill remains unchanged. Linux residency tests
query the full mapping before PCM access; macOS and Linux tests check sample-bit
preservation and allocation-free callbacks. Prefaulting does not permanently
lock pages in memory or guarantee delivery after a missed graph deadline.

A separate Debian 13 x86_64 KVM guest (2 vCPUs, 4 GiB; Linux 6.12.111,
PipeWire 1.4.2, WirePlumber 0.5.8; server/client FIFO 88/83) passed the current
32-channel PCM24 and 64-channel PCM16 continuity cases three times each.
Each case retained 480,255 indexed frames with zero continuity errors in each
of two independent client scopes. Separately built HEAD passed both cases once.
The reference PipeWire loopback and all-format negotiation/removal cases passed.
This finite environment comparison does not qualify delayed-reader recovery,
same-device feedback, or long-duration operation. Hardware, architecture, kernel,
hypervisor and PipeWire build differ from the Docker lab; see `docs/product.md`
for provenance and the excluded shared-build-cache comparison.

The environment-dependent midstream PCM loss remains a known issue. The current
release decision does not treat these reproductions alone as a blanket Linux
release blocker. Arch Linux and long-duration operation on user hardware remain
unqualified; a reproduction during normal use in a supported environment requires
reassessment. Keep all continuity assertions and failure thresholds unchanged.
