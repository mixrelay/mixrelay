# Shared device signal and asynchronous format acknowledgment

## Scope

- A virtual device combines OS application playback and all graph writes. Graph
  readers and OS capture receive that device mix; there is no source-kind selector.
- Do not detect, reject, mute, or automatically correct feedback through a device.
  External application routing is not observable to the engine. No SCC analysis,
  feedback-dependent delay insertion, or feedback recovery policy was added.
- The native transport retains its actual render/capture channel widths. The
  combined bus uses their maximum width and projects onto the capture channels.
- Every virtual device uses the same preallocated completed-block snapshot for
  graph reads. This ordinary device boundary is independent of topology. Direct
  hardware routes remain in the current callback. Snapshot state is cleared on
  clock/source discontinuity, and short blocks cannot expose stale tail samples.
- Endpoint sample-rate setters wait for native acknowledgment on the control
  worker. Old-rate and temporarily unavailable observations are pending, not
  immediate failures. Rollback supersedes pending HAL changes before resuming.
- Device identity is observed once per catalog update and applied to every graph
  reference. A same-revision graph switch cannot render the previous document.
- Reconciled negotiation errors clear only after checking catalog persistence;
  a genuine persistence failure remains visible.

## Executed checks

- Rust library suites: audio-engine 28, audio-graph 25, engine-service 184;
  **237 passed, 0 failed, 0 ignored**.
- macOS binary tests (`macos::` filter): **44 passed, 0 failed, 0 ignored**.
  Old `readSide` JSON in three native test fixtures was removed. Native ownership
  fixtures explicitly prepare internal transport nodes rather than bypassing
  the authored-device lowering contract.
- Strict Clippy for audio-graph, audio-engine and engine-service, all targets:
  passed with `-D warnings`. Rustfmt check passed.
- Earlier focused UI suites: 224 passed. Final rate-hook, StudioScreen and routing
  model rerun after locale cleanup: **23 passed**. Type checking and UI boundary
  checking passed. Desktop debug app and engine debug binary built successfully.
- Vite still reports the existing bundle-size warning (>500 kB); this is not a
  warning-free frontend bundle or a bundle optimization claim.
- Independent read-only review covered device mixing, native channel ownership,
  callback allocation, epoch reset, rate rollback and catalog persistence. Review
  findings were resolved and the corresponding regression tests executed.

## Local app verification

The existing launch daemon and the debug MixRelay app were restarted with the new
engine/UI. No HAL installation or CoreAudio restart was performed.

- The two existing graphs, endpoint ID/name, channel routes, gains, node positions,
  viewport and layout revisions were preserved. Only removed source-kind fields
  and the VIO source port representation were updated in the stopped settings file;
  opaque edge IDs were retained. No backup or compatibility reader was created.
- From Graph 2, the actual sample-rate dropdown changed **96 -> 44.1 kHz**.
  Native observation, Graph 1's dropdown, Graph 2, and both saved graph documents
  agreed on 44.1 kHz. Neither screen showed a negotiation failure.
- From Graph 1, the dropdown restored **96 kHz**. Both stored references and the
  native device reported 96 kHz. The original buffer size **32** was retained;
  the hardware-clock processing format remained 48 kHz / 32.
- Graph 1 microphone and device-write peaks matched (0.004748019 on each routed
  channel). After switching to Graph 2, device-read and headphone-output peaks
  matched (0.0046242913 per channel). The two snapshots occurred at different
  times; they are not an end-to-end waveform/null test.
- Final UI has no source-kind selector and labels the direction as device-to-graph,
  not application-only. Final status: ready/running, issue null, Graph 2 selected.
- Semantic comparison after verification confirmed all graph contents and layouts
  retained their initial values apart from document/catalog revision progression
  and the removed source-kind representation.

## Limits and timing evidence

- No feedback topology was created in the user's audible routing for testing.
- Unit tests establish exactly one completed I/O block at the VIO graph-read
  boundary (32 / 48000 = 0.667 ms in the current processing format), zero added
  block delay on direct hardware routing, and zero heap operations in the tested
  render path. This is not measured acoustic or application round-trip latency.
- No before/after performance improvement is claimed: the previous implementation
  split signals that are now combined, so it does not provide equivalent routing
  semantics for a performance comparison. The alternative evidence is exact PCM,
  bounded snapshot behavior, allocation assertions and the live routing checks.
- Underrun/overrun counters were already 566/8 at the first post-startup status
  sample and remained 566/8 throughout the rate round-trip and graph switching.
  This is not proof of zero startup underruns or artifact-free playback. Listening,
  calibrated loopback and long-duration drift/leak measurements were not performed.
- The previous engine's shutdown logged a native-owner quarantine after a 2-second
  teardown timeout. The new engine started and ran, but this check does not certify
  that separate teardown path leak-free.
