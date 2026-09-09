# Windows native render-demand pacing — 2026-09-06

This qualifies a bounded Windows ARM64 boundary, **not full Windows support or a
general absence of leaks**. It follows the earlier
[activation/lifetime fixes](2026-09-06-windows-native-validation.md).

## Implemented boundary

- With a hardware output attached, the graph no longer independently generates
  128 frames every 2.667 ms. The first output in stable node-ID order supplies
  native demand after successful WASAPI buffer submissions. The graph refills
  those frames through a bounded SPSC ring, one native quantum ahead.
- The native worker owns COM and the WASAPI buffer. A cumulative atomic frame
  count and coalescing thread wakeups notify the single graph owner; a timeout
  only checks shutdown/graph edits. It never authorizes new audio frames.
- The count uses negotiated stream frames, not `IAudioClock` tick units. No
  wall-clock timestamp is passed off as a native timestamp. Prepared-stream
  backlog is discarded on attachment; stalled catch-up is bounded and marked
  discontinuous. Oversized native buffers are rejected at preparation.
- Variable native demand is split by `CycleBlocks` without dropping remainders.
  Clock identity changes explicitly on selecting a different stream.
- With no output, the existing capture/control-only timer remains explicitly
  unsynchronized. Removing the output resets that timer's timeline. A failed
  primary output does not silently turn into a timer-backed healthy output.
- Runtime status reports the active pacing authority. Independent capture and
  secondary output clocks still require drift correction; that capability stays
  false. This work adds a native-quantum/ring handoff delay, not a low-latency claim.

The buffer contract follows Microsoft's
[shared-event rendering](https://learn.microsoft.com/en-us/windows/win32/coreaudio/rendersharedeventdriven)
and [GetCurrentPadding](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-getcurrentpadding).
Clock ticks must not be mistaken for frames; see
[IAudioClock::GetPosition](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclock-getposition).

## Executed validation

Windows 11 ARM64 in the existing UTM VM, interactive user session 1, MSVC ARM64
14.44.35207 and Rust 1.96.0. No host audio defaults, security policy, driver signing,
or guest login configuration were changed.

| Check | Result |
| --- | --- |
| `windows-native-v8` | Engine build, **48** focused native tests and Clippy including test targets; exit 0 |
| Pure demand/quantum contract | 10,000 variable-demand observations; exact frame conservation, remainder preservation, no processing for unchanged native count |
| Clock-boundary regression | Attachment, stream change, delayed/bounded catch-up, counter restart and timer reset passed |
| Actual production graph worker | Revisions 2–9: four native-output attachments and four removals; graph-to-native ring writes and empty-graph return passed |
| `windows-audio-v6` | Both opt-in tests passed, **86.34 seconds**, exit 0 |
| Capture/render/teardown | **120** complete cycles; own-process capture only; 31,680–34,080 captured frames per cycle; every peak exactly **0.004 / 0.002** |
| Native-paced writes | 33,600–34,080 frames per cycle; every DSP-sized write accepted fully |
| Handles after teardown | 152 initially, 154 from cycle 7 through cycle 119; no per-cycle accumulation observed |
| Private committed bytes after teardown | 2,318,336–2,576,384 overall; last 30 cycles 2,396,160–2,539,520 (140 KiB range); final 2,404,352 |
| Host checks | Windows x64-target Clippy; 41 local focused Rust tests; 24 lab helper tests passed |
| Final inventory | `final-state-v3` exit 0; owned task absent, no owned engine/test process, both validation markers checked as zero |

The production graph test sends deterministic silence through a connected
processor/output path, avoiding self-capture feedback. The separate signal test
exercises the real native demand/capture/render path with an owned quiet tone.
It verifies peaks/channel ratios, not a full end-to-end application's waveform.

The first graph fixture (`windows-audio-v5`) waited for writes to an **unconnected**
sink, which the DSP correctly omits from external bindings. The fixture timed out;
its separate 120-cycle signal test passed. The fixture now includes an actual
route and the expected writes are observed. The failed job/log is retained;
neither its timeout nor its overall exit code is relabeled as a pass.

## Measurement scope and remaining work

This is a scheduling/ownership correctness change. No CPU, throughput, latency,
or measured drift-improvement percentage is claimed. A controlled old/new timing
benchmark was not performed: the live harness was changed from timer-fed writes
to native-demand-fed writes and is not an identical performance workload. Frame
conservation tests and the native lifecycle/graph tests are the alternative
correctness evidence. These memory samples are a bounded churn observation,
not hours-long steady-state, plugin, kernel-pool, or all-allocation leak analysis.

Remaining: asynchronous source/secondary-device SRC and drift control, capture-only
native pacing, selectable clock/format negotiation, Windows x64 execution, real
hardware continuity/latency and long endurance. The Windows virtual driver is
still a transport header only: WaveRT miniport, INF, safe kernel transport,
installation/signing and driver qualification have **not** been implemented here.

Evidence is retained under `/Volumes/Linear 1/test/tonenari/evidence/`:
`windows-native-v8-20260906.log`, `windows-audio-v5-20260906.log`,
`windows-audio-v6-20260906.log`, and `windows-final-state-v3-20260906.log`.

The first final-state inventory ran before asynchronous guest task cleanup had
completed and correctly failed. Cleanup now writes its own completion marker and
verifies the task is absent; final inventory is dispatched only after that marker
is zero. Guest-exec dispatch success is not treated as command completion.

Validated source archive SHA-256:
`efc9dbda03f69997669f89680f265f91e34e096a9c5f588f67f97c978f4ea7f5`.

Validated guest engine SHA-256:
`4deb11b9e8b0805d99eec7d24579536152b9bf8d871f9b46a4b9960120f633f3`.
