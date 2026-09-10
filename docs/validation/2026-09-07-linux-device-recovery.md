# Device loss / arrival recovery — 2026-09-07

## Implemented, not just a capability flag

The previous Linux device implementation silenced disconnected streams but never
reopened them for an unchanged graph. A dedicated control worker now observes
per-stream atomic health, resolves the original selector, prepares replacements
and hands them to the audio callback through a bounded generation-tagged mailbox.

- A recovery swaps only disconnected slots. Healthy inputs/outputs retain their
  existing native stream serials; application/virtual resources are not reopened.
- Inputs and outputs can return independently. An unavailable device does not
  prevent another selected device from recovering.
- Device-generation changes invalidate old prepared results. Deleted nodes cannot
  be resurrected by late discovery or connection completion. Recovery does not
  change graph revisions, history, cables or gains.
- Input rings discard pre-activation PCM and re-prime, rather than replaying
  microphone samples queued while recovery was preparing.
- Native preparation, rejected/stale owners and retired streams are destroyed off
  the audio callback. RT work only observes atomics, finds existing slots and swaps.
- The recovery worker is cancelled and joined during shutdown. Cancellation is
  checked between device operations and during stream-ready waits.
- Each `pw-dump` / `wpctl` call has a two-second timeout with nonblocking stdout /
  stderr, capped at 16 MiB / 256 KiB. Failure kills/reaps only the owned child, without
  leaving reader threads. Missing/ambiguous/wrong-width devices still do not fall
  back to unrelated devices.

`audio-io::recovery::RecoveryMailbox` is OS-independent ownership infrastructure.
Linux is wired to it in this slice. Its existence does **not** mean macOS and
Windows have received the same native recovery implementation.

## Tests and native proof

Portable tests cover stale-generation rejection, failed publication ownership,
control-thread destruction and 1,000 handoff/reclaim cycles. Linux tests cover
per-stream health filtering, cancellation before native opening, bounded command
timeout, oversized output, and draining 2 MiB without truncation or pipe deadlock.
Native ring tests cover 1,000 discard/re-prime cycles under ASan/UBSan, in addition
to existing channel/quantum/wrap/overflow/concurrency checks.

Ubuntu ARM64 uses the owned `MRDeviceFixture` source/sink. SIGUSR2 and SIGHUP remove
and recreate the two devices independently under the same names, with fresh native
serials. No system defaults or existing user engine are changed.

Native v3 and v4 passed:

1. All prior 2/8/16/32/64-channel PCM routes, 64→16 channel 13/33 mapping, summation,
   gain, meters, rejection and shutdown checks.
2. Four input disconnect/reconnect cycles without editing the graph. The healthy
   output serial remains unchanged. In v3, observed recovery took **132–355 ms**
   (500 ms retry cadence plus graph/registry/meter observation).
3. Output-only loss while input continues, and input return while output is still
   absent. Returning the output does not recreate the already recovered input.
4. Gain changed to −12.0412 dB while input was absent. Recovery preserves that
   edit; independent sink PCM measures channels 1/16 at **0.003173829 / 0.008056642**,
   all unconnected channels zero. v3 records 49,803 active frames, no invalid samples,
   and a one-frame maximum internal silent run (sine zero crossing).
5. Deleting nodes while input is absent, then restoring the device: no nodes or
   cables reappear, and the empty graph revision stays unchanged.
6. 32 create/remove cycles followed by clean engine/fixture shutdown, with no owned
   PipeWire nodes remaining.

The cancellation follow-up passes **66 Linux Rust tests** (15 graph, 22 audio-io,
25 engine library, 4 native-adapter unit tests) and Clippy in v5. Its native audio
checks pass but RSS fails, as recorded below. The subsequent PCM slab fix passes
the full native probe and native sanitizer suite in v6.
macOS portable/engine regression: **56 tests passed**. Windows ARM64 regression:
**70 passed, 2 interactive WASAPI tests ignored**. Both pass affected Clippy checks.
These Windows/macOS runs are regression evidence, not native recovery certification.

## Memory findings, including the failed run

v2 passed recovery/PCM but failed the earlier 12-cycle RSS range assertion, sampled
immediately after native node disappearance. Node removal can precede completion
of deferred destruction. The original failed harness did not save the RSS array;
that evidence gap is corrected now. This run is retained as failed, not relabelled
as leak-free or explained away as proven allocator caching.

The revised measurement waits 550 ms after native node removal, covering the
500 ms recovery-retirement interval, uses 32 cycles and retains the original
8 MiB post-warmup range gate. All samples are saved before assertions.

| Same measurement conditions | Post-warmup RSS | Range |
| --- | ---: | ---: |
| v3, 32 cycles | 100,960–100,964 KiB | 4 KiB |
| v4, 32 cycles | 101,376–101,380 KiB | 4 KiB |

Those two successful measurements were insufficient: v5, with the same wait and
32-cycle workload, grows from 100,044 to 111,912 KiB after warmup and fails the
unchanged 8 MiB gate. Its complete RSS array is retained. The failure is not waived.

Large native PCM ring slabs now use zero-initialized anonymous mappings and
explicit `munmap` on control-thread retirement, instead of `calloc/free` whose
released blocks can remain in allocator arenas. This makes returning each slab to
the OS part of its ownership contract. DSP callbacks still never map/unmap storage.

| Identical 32-cycle harness, 550 ms settling interval | Final RSS | Post-warmup range |
| --- | ---: | ---: |
| Before, v5 (`calloc/free`) | 111,912 KiB | 11,868 KiB — failed |
| After, v6 (`mmap/munmap`) | 57,236 KiB | 1,612 KiB — passed |
| Repeat, v7 (`mmap/munmap`) | 57,052 KiB | 1,612 KiB — passed |

Final RSS falls **54,676 KiB (48.9%)** in this paired workload. Peak values, channel
mapping, recovery and ring sanitizer checks still pass. This demonstrates reduced
retained resident memory, not proof of the exact cause of every allocator growth
step and not a universal leak-free guarantee. v2 and v3 timings differ and cannot
support a before/after comparison; the measured comparison is v5 versus v6.
Recovery latency above is acceptance measurement, not a speedup benchmark ratio.

Remaining gates: real hardware unplug/arrival, whole PipeWire server restart,
long-duration resource/clock stability, Debian/Kali/Arch native sessions, and native
feature parity on macOS/Windows. Existing Windows product-driver and processor/
editor qualification gaps remain unchanged.

Evidence: `/Volumes/Linear 1/test/mixrelay/evidence/linux-recovery-20260907`.
`failed-v2.log`, `v3-tests-native.log`, `v3-recovery.json`, `v3-result.json`,
`v4-result.json`, `windows-regression.log`, `failed-v5-tests-native.log`,
`before-v5-rss.json`, `after-v6-result.json`, `after-v6-recovery.json` and
`after-v6-native.log` are retained. The current probe records failure data before
asserting and never removes a failed run.
The final unchanged repeat also passes, exit 0: `after-v7-native.log`,
`after-v7-result.json`, `after-v7-recovery.json`. Engine/fixture ownership checks
finish with no retained PipeWire nodes in both mapped-slab runs.
