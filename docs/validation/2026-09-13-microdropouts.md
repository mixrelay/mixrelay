# Micro-dropout capture and buffer repair — 2026-09-13

Status: **not accepted as an audible-continuity fix yet**. Actual pre-repair PCM
contains gaps. The new source passes the focused checks below, but the matching
post-repair recording is blocked by native device registration/property calls.
No system-wide CoreAudio reset, HAL driver installation, commit or push was performed.

## Measurement boundary and preserved state

- Local checkout: `/Volumes/dev/MixRelay`, `feat/audio-routing-and-recorder`.
- Existing debug desktop bundle and app-owned engine; no alternative audio server.
- Selected editor graph remains `graph-105`. Measurement explicitly reads
  `graph-1` without selecting it or loading a replacement document.
- Confirmed retained documents after the failed restarts: graph-1 revision 672,
  three nodes/four edges; graph-105 revision 675, two nodes/two edges.
- Graph-1 microphone is 96 kHz mono, gain 0 dB, feeding both capture channels of
  the referenced MixRelay IO. Its built-in-speaker node is OFF. Graph-105 reads
  the same virtual device and routes to the 96 kHz headphone output.
- Measured processing/virtual format: 48 kHz, DSP block 32 frames. No graph,
  node, gain, default device or sample-rate preference was changed for the test.
- The recorder attaches an independent native CoreAudio input IOProc to the
  actual referenced virtual-device UID. It does not insert a graph recorder.
  PCM is preallocated and copied in the callback; WAV I/O happens only after
  stop/unregister has drained the callback. No audio was uploaded.

The capture is the **OS-visible MixRelay capture bus**, not an analog recording
of the headphone DAC. Native microphone/reference and destination were not both
captured with this native fixture, so end-to-end latency is not measured yet.

## Actual PCM baseline

Artifact directory: `output/microdropouts-20260913-1310/` (local, unstaged).
The directory suffix is an identifier, not the capture start timestamp.

| Native capture, 30.009 s wall time | Channel 1 | Channel 2 |
| --- | ---: | ---: |
| Captured frames | 1,440,256 | 1,440,256 |
| Payload duration | 30.0053 s | 30.0053 s |
| Exact-zero frames | 5,629 | 5,629 |
| Exact-zero runs | 50 | 50 |
| Longest exact-zero run | 752 frames / 15.6667 ms | 752 frames / 15.6667 ms |

Native callback evidence: 2,813 callbacks, zero malformed callbacks, zero
timestamp gaps. The same graph revision/format remained in place. During the
window, engine counters changed by 0 underruns, 257 overruns and 257
discontinuities. The integration test correctly **failed**. Its post-close
status also exposed a rapidly increasing false capture-admission error count.

Files: `native-before.ch1.wav`, `native-before.ch2.wav`, and their corresponding
`.analysis.json` files. The analyzer's WAV packet timestamps are file coordinates;
native callback timing continuity is proven by the integration fixture, not by
WAV timestamps alone. Exact-zero runs are measurements, not a complete detector
for every possible audible click or upstream microphone fault.

An earlier paired AVFoundation/FFmpeg recording is retained as `capture.nut`,
`capture.log`, `observations.json`, `avfoundation.analysis.json`. Its delivered
physical-microphone PCM had no exact-zero frames, but the recording adapter lost
2,436 timestamp ranges on the mic and 285 on MixRelay. Its missing timestamps
**must not be counted as engine dropouts**, and it is unsuitable for a precise
paired latency comparison. The native capture above replaced that evidence path
for continuity acceptance.

## Repairs and ownership

1. A contended callback-demand seqlock read is `SnapshotError::Pending`, not
   device removal. Primary pacing preserves partially consumed callback credit.
   Independent capture/render boundaries now also retain converter history and
   queued PCM during Pending; capture publication waits for coherent demand.
   Missing mappings and real identity/epoch/rate transitions still invalidate.
2. PCM identity checks use stable lifecycle atomics, not the independently moving
   callback-demand seqlock. A concurrent demand publisher no longer rejects
   otherwise valid PCM. Epoch, incarnation, format and lane checks remain.
3. Closing an OS capture client leaves its last demand stamp. A full reservation
   window is ordinary backpressure, not a failed clock or a reason to stop all
   HAL-driven DSP. Actual preparation/publication failures remain errors.
4. Each native connection has a sole control owner and one scoped PCM worker.
   Synchronous CoreAudio property/liveness IPC runs only on the control owner;
   the PCM pump owns the converter and its FIFO cursors. Cancellation or failure
   joins the PCM worker before disconnect/destruction. No detached converter,
   callback task submission, or concurrent cursor mutation is introduced.
5. Transport-loss recovery resets prepared converter/FIFO history and publishes
   a new epoch without reconstructing sinc tables or allocating scratch buffers.
   Actual format changes still prepare a converter for the new geometry.
6. Microphone drift feedback accounts for native input backlog, converter-staged
   frames, and graph-output backlog converted into source-rate units. The former
   destination-only observation was capped by the pump and could not see input
   accumulation. One controller uses actual source progress; no amplitude-based
   clock inference, hidden packet discard or fixed large latency target is used.
7. One-second `audio_trace` snapshots report native/graph progress, FIFO lengths,
   epochs, overflow/clock-gap counts and HAL demand. The format monitor reads
   atomics on the control side; native callbacks do not log or perform file I/O.

The microphone feedback target derives from the observed native/graph burst plus
one converter input slab. The 16,384-frame ring remains a safety capacity, not a
desired backlog. The trace/profile baseline observed that ring filling; at
96 kHz its capacity alone represents 170.7 ms, excluding all other path delays.

## Same-condition focused measurements

All measurements ran serially without custom RUSTFLAGS/target-cpu flags. The
source changed during development, but each paired microbenchmark uses one
compiled test, the same native synthetic owner and geometry, and no other Cargo
test/benchmark running concurrently. These are not whole-engine speed claims.

### Converter recovery

Debug build, 96 kHz -> 48 kHz stereo, 100 recoveries per round, six rounds with
alternating execution order. Both paths activate/reset the same native owner;
the prior path replaces/drops its converter, the new path reuses its storage.

| Per 100 recoveries | Reconstruct converter | Reset prepared converter |
| --- | ---: | ---: |
| Median elapsed | 73,478 us | 25 us |
| Allocation count | 27,000 | 0 |
| Allocated bytes | 67,041,400 | 0 |

Timing is computational recovery cost with the test logging subscriber inactive,
not live callback latency. The zero-allocation regression also checks unchanged
scratch addresses, a fresh epoch, and no replay of pre-reset PCM.

### Drift/backpressure

60 synthetic seconds per case, 96 kHz source -> 48 kHz graph; 32-frame native
callbacks, batched 256-frame graph consumption, identical priming and drift.

| Source drift | Old peak/final native backlog | New peak/final native backlog | New correction |
| --- | ---: | ---: | ---: |
| -300 ppm | 2,432 / 384 | 2,432 / 0 | +310.995 ppm |
| 0 ppm | 2,432 / 2,432 | 2,432 / 0 | +2.407 ppm |
| +300 ppm | 4,160 / 3,968 | 2,560 / 0 | -311.434 ppm |

All cases transferred PCM without frame loss/overflow. Old correction stayed at
0 ppm and failed the drift/backlog assertion. New cases satisfy the unchanged
loss/overflow/backlog/correction criteria. Zero final native backlog does not
mean zero total latency: converter and graph-output frames remain accounted for.

An earlier source-only feedback experiment was rolled back after the user heard
worse results; rollback did not eliminate the live errors. The final change
above measures total pending input rather than either isolated FIFO. It has not
yet earned a live improvement claim.

### Concurrent demand publication

The 10,000-transfer synthetic contention regression previously rejected 8,633
reads and 8,375 writes. With lifecycle-only identity validation, both rejection
counts were 0. This deliberately contended fixture does not represent a measured
production dropout rate.

## Validation results and limits

Passed on the updated source:

- `cargo fmt --all --check`, `git diff --check`.
- Strict Clippy, all targets, for engine-service and macOS transport.
- Engine-service debug build; `cargo doc` for engine-service/macOS transport.
- macOS execution/boundary tests: 81 passed; pacing: 8 passed.
- Engine endpoint service: 21 passed; endpoint transport: 15 passed.
- Shared clock bridge: 13 passed, including prepared-path allocation checks.
- Synthetic device tests: 15 passed, including native owner retirement stress.
- Drift/backpressure regression and reset storage/PCM regression passed.
- Scoped PCM worker tests cover blocked control, PCM failure, normal stop and
  control-side unwind/join (3 tests).
- WAV header/sample-bit export regression passed. Both real WAVs were decoded
  and analyzed independently; Node capture script syntax checked.

The broader `macos_devices::` run was **73 passed, 6 failed, 1 explicit timing
benchmark ignored**. The six failures require dedicated installed fixture UIDs
(`MIXRELAY_TEST_ENDPOINT_UID` / aggregate output UID), which are not configured.
They were not disabled or rewritten to report success. The timing benchmark was
run explicitly and passed. Full workspace/integration acceptance is not claimed.

Post-repair live capture is blocked: app-only restarts at 13:21 and 13:31 UTC
both deferred graph restoration after microphone initialization timed out. The
sampled stack was inside `AudioDeviceCreateIOProcID`; a later stack was inside
CoreAudio property IPC during device inventory. The microphone owner returned
only minutes later. Profiles are retained as `startup.sample.txt` and
`startup-late.sample.txt`. The earlier running profile is `engine.sample.txt`.
This is observed blockage at an OS call, not proof that the OS alone caused it.
The desktop remains open with restoration deferred; saved graphs are retained.
The Mac was then found locked, preventing further UI inspection.

## Remaining acceptance work

- Restore native I/O availability, then rerun the same 30-second native capture
  with a fresh `MIXRELAY_CAPTURE_OUTPUT` prefix. Do not overwrite the baseline.
  Compare PCM, callback timing and engine counters, including capture close/resume.
- Capture an independent native microphone reference alongside the destination
  for a true paired latency measurement; add analog headphone loopback if needed.
- Stress Stream Deck power/gain controls and graph editing while recording;
  confirm no options collapse, graph reload, clock switch or unrelated owner loss.
- Mixed-rate hardware still has a worker-pacing path. This patch does not finish
  the broader callback-authority redesign or prove deadline compliance at 32
  frames. Lost native credit must remain visible, not hidden with a larger FIFO.
- macOS virtual-output volume/mute controls are not implemented by these changes;
  driver installation and native UI verification remain separate unfinished work.

Reproduction command (after native availability is restored):

```sh
MIXRELAY_TEST_INSTALLED_ENGINE=1 MIXRELAY_TEST_GRAPH_ID=graph-1 \
MIXRELAY_CAPTURE_OUTPUT=/Volumes/dev/MixRelay/output/microdropouts-20260913-1310/native-after \
cargo test -p mixrelay-engine-service --test macos_installed_continuity \
  installed_mixed_route -- --nocapture --test-threads=1
```
