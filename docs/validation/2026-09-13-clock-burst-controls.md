# CoreAudio callback-burst buffering and external controls

## Scope and findings

The reported warning was `clock_not_synchronized`, not a graph-file synchronization error.
Changes are limited to the CoreAudio worker/device boundary and a UI regression test.
Existing uncommitted feature work and user graphs/settings were preserved; no commit or push.

- A pacing callback can supply multiple 32-frame DSP blocks. The hardware bridge previously
  sized its destination queue from the last **DSP block**, not the full pacing callback.
  With a 512-frame HAL callback, its 64-frame input queue could not supply that burst.
  Native capture accumulated until its 16,384-frame queue overflowed, repeatedly resetting SRC.
- The selected pacing callback's quantum now reaches the bridge via an owned atomic. Hardware
  quanta are converted into graph-rate frames; HAL quanta retain their native callback extent.
  Queue policy still retains one verified shared-clock burst or two independent-clock bursts,
  bounded by the existing capacity. No device sample rate or DSP block-size setting is changed.
- Worker polling is bounded to one quarter of the destination queue's time span (maximum 1 ms).
  OFF directions retain the previous 1 ms control polling interval and perform no conversion.
- OFF hardware directions are excluded from PCM clock-health checks, while the native clock
  identity remains available for pacing. Re-enabling still requires actual bridge-ready evidence.
- Activation logs now include overflow/discontinuity counts and native/DSP/pacing quanta.
  These logs run on resource workers, never native audio callbacks.

## Reproducible same-input regression

`split_pacing_burst_retains_the_complete_callback_not_one_dsp_block` compares the old 32-frame
budget with the published 256-frame pacing budget. Both process eight 32-frame graph reads,
the same mono PCM, identical rates and converter settings, without wall-clock scheduling.

| Native -> graph rate | Valid input with old budget | Valid input with callback budget |
| --- | ---: | ---: |
| 48,000 -> 48,000 Hz, verified shared clock | 32 / 256 frames | 256 / 256 frames |
| 96,000 -> 48,000 Hz, independent SRC | 64 / 256 frames | 256 / 256 frames |

This is a transfer-correctness comparison, not a CPU-throughput benchmark. Additional checks
cover bounded polling, OFF health exclusion, retained clock identity, required ON priming,
and zero retained native owners after 512 create/retire cycles (allocator reported zero leaks).

## Live observations, 2026-09-13 UTC

Same Mac, debug profile, 32-frame DSP, 48 kHz processing, built-in 96 kHz microphone,
48 kHz speakers, 96 kHz headphones, two user graphs and a 48 kHz virtual endpoint.
Graphs included microphone -> speaker/virtual capture and virtual playback -> headphones;
an AUNBandEQ node was also retained. Graph selection is not an execution switch.

Source: `~/Library/Logs/MixRelay/engine.stderr.log`. No unrelated system audio service was reset.

| Interval | Bridge activations | Input activations | Rendering gates | Native reconnect starts |
| --- | ---: | ---: | ---: | ---: |
| Before, 12:05:00–12:06:00 | 276 | 269 | 1 | 0 |
| After, 12:13:00–12:14:00 | 5 | 1 | 4 | 0 |
| Following steady interval, 12:14:00–12:15:00 | 0 | 0 | 0 | 0 |

The first after interval includes four hardware/HAL clock-alignment transitions and one input
overflow recovery; it is **not** a claim of zero startup/clock-transition loss. Overall activation
count fell 98.2% in that first minute. The following full minute was quiet. These live windows
are diagnostic observations under a real OS scheduler, not an isolated CPU benchmark; the
deterministic transfer test above supplies the exact same-input comparison.

At 12:16:03.980–12:16:14.921, the installed app's local control socket (the Stream Deck path)
accepted 20 microphone gain commands (-0.5 / +0.5 dB) and 20 speaker power commands.
Revision advanced 617 -> 657; structure generation stayed 616; `requiresGraphReload` was false
for all 40 commands. There were zero native reconnect starts, zero input resets, zero rendering
gates, and zero WARN/ERROR log entries. Twenty output activation entries were the expected
OFF/ON FIFO invalidations, not device reconnections. Final clock issue was null. Microphone
gain returned to 0 dB and speakers to OFF.

A cold first speaker enable before that run required native preparation (`requiresGraphReload`
true once, revision 616); it is not included in the warmed-control zero-reload count.

The microphone Options panel stayed expanded throughout those 40 commands in the real app.
The earlier one-time closure did not reproduce in this session. No speculative UI lifecycle
change was made. A regression now verifies the same mounted slider and open disclosure across
four external gain revisions, and verifies that an intentional background click still closes it.

The final frozen build was restarted through normal app quit/open. At 12:23 UTC it retained
both graphs, revision/activeRevision 659, 48 kHz/32 frames, and clock issue null. Its first
microphone -0.5 dB command acknowledged revision 660 without graph reload; native UI inspection
showed the same expanded Options and slider at -0.5 dB. The following command restored 0 dB
at revision 661, also without graph reload. The app remains open with microphone Options shown.

## Validation and limitations

- `cargo fmt --all -- --check`: passed.
- Strict Clippy for engine-service binary and tests: passed (`-D warnings`).
- Engine-service debug binary build and package documentation build: passed.
- Focused CoreAudio device tests: 15 passed, including both added regressions.
- Engine binary suite: 159 passed, 6 failed at fixture setup. Five `macos_devices::live_tests`
  require `MIXRELAY_TEST_ENDPOINT_UID`; the aggregate lifetime test requires explicit primary
  output/test device UIDs. Those fixture variables are not configured. No tests were deleted,
  ignored, or changed to conceal that limitation, and a user's live endpoint was not repurposed
  for destructive rate/hotplug fixtures. This is not a full-suite pass.
- UI canvas, node-control queue and reconciliation tests: 38 passed.
- UI TypeScript and architectural boundary checks: passed.
- `git diff --check`: passed; Git index remained unchanged.

Ownership review: the new quantum atomic shares each Device/WorkerContext lifetime and is
published only by the serialized graph owner. No new lock, allocation, OS property query,
PCM cursor owner, raw-pointer API, or independent control subsystem was added to graph render.
Native snapshot reads stay within existing retained FFI ownership; converter mutation remains
worker-owned. The user requested no delegation; review and validation were performed by the
primary agent after interrupting the earlier read-only subtask.
