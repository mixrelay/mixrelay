# Linux native format contract and isolated tests

The session sample rate is the ToneNari-owned stream's negotiated F32 PCM rate.
The PipeWire server graph clock is independent of that setting. Its adapters
perform sample-rate conversion; `GraphTimeline` / `FrameBudgetMapper` convert
callback duration to render frame units, not PCM. Adding another PCM converter
at this boundary would resample audio twice.

The UI processing buffer is the maximum ToneNari DSP block (`CycleBlocks`).
`node.latency` requests a scheduling preference but does not set the server's
quantum or another application's buffer. No global metadata is changed. Actual
variable callback quantum is an internal observation, never an
`observedDeviceBufferFrames` value or a reason to overwrite the selected buffer.

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

Run only in a uniquely named `<checkout>/.lab/tonenari-integration-*` source snapshot
inside the lab guest. The harness creates a private runtime, config, state and
D-Bus, disables hardware monitors, then reaps only its owned processes. It does
not alter services, default devices, system configuration, or user graphs.

```sh
TONENARI_LAB_GRAPH_RATE=44100 TONENARI_LAB_SOURCE_RATE=48000 \
  TONENARI_LAB_TEST_NAME=native_pipewire_rate_conversion_and_observed_clock \
  bash native/linux/pipewire/tests/run-application-retracking.sh "$snapshot"
TONENARI_LAB_TEST_NAME=native_pipewire_live_session_format_transaction \
  bash native/linux/pipewire/tests/run-application-retracking.sh "$snapshot"
TONENARI_LAB_LIFECYCLE_TEST=1 \
  bash native/linux/pipewire/tests/run-application-retracking.sh "$snapshot"
```

The lifecycle test uses only its own source and observer. It deliberately queues
obsolete PCM around native state transitions to check the production epoch
handoff. Keep its result separate from ordinary ring unit tests; an unexecuted or
failing native fixture is not lifecycle acceptance.
