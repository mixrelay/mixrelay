# Mixed-clock continuity — 2026-09-14

Follow-up implementation and measurements are recorded in
[callback-owned SRC](2026-09-14-callback-owned-src.md). The results below describe
the preceding attempt and remain failed observations, not the current architecture.

Status: **not accepted as an audible-continuity fix**. Microphone permission is
restored, but deadline-facing input shortages remain. No graph/rate/default-device
settings, HAL installation, system-wide audio restart, commit or push was performed.

## Preserved conditions and measurement

The same app-owned debug engine, 48 kHz processing / 32-frame DSP, and independent
native HAL capture fixture were used in three sequential 30-second recordings.
Graph 1 has a 96 kHz mono built-in mic at 0 dB, feeding the 48 kHz built-in speakers
and both channels of MixRelay IO. Graph 2 reads that virtual bus into 96 kHz
headphones. Nodes are enabled and unmuted. The app remained the resource owner.
Restart restoration advanced graph-1 revisions 686 -> 688 -> 690; the recorded
node/edge/control configuration remained the same. No user graph was replaced.

Artifacts: `output/microdropouts-20260914-1105/`, local and unstaged. Each prefix has
independent `ch1.wav` / `ch2.wav` PCM; channel 1 also has an analyzer JSON report.
No recording was uploaded. This records the OS-visible virtual capture bus, not
the headphone DAC or an independent simultaneous microphone reference.

| 30-second run | Before | Direct hardware clock only | Direct + converter quantum |
| --- | ---: | ---: | ---: |
| Prefix | `before` | `after-direct` | `after-quantum` |
| Frames per channel | 1,440,256 | 1,440,256 | 1,440,256 |
| Native callback timestamp gaps | 0 | 0 | 0 |
| Invalid/malformed PCM | 0 | 0 | 0 |
| Exact-zero frames, channel 1 | 800 | 5,942 | 0 |
| Exact-zero runs, channel 1 | 5 | 17 | 0 |
| Longest exact-zero run | 480 / 10 ms | 1,090 / 22.708 ms | 0 |
| Engine underrun increment | 0 | 1,245 | 148 |
| Engine overrun increment | 4 | 4 | 0 |
| Engine discontinuity increment | 6 | 0 | 0 |
| Installed continuity test | FAIL | FAIL | FAIL |

These are repeat live observations, not a claim of deterministic performance or
perfectly repeated acoustic input. All native recordings had 2,813 callbacks.
Exact-zero removal is not proof that all clicks disappeared: sinc filtering can
spread a shorter source gap into nonzero samples. The remaining 148 underruns
therefore keep acceptance failed. The intermediate direct-only regression is
retained in this report and its PCM, not hidden by the later result.

## Changes and ownership

- Hardware frame credit now has priority over a virtual client's activity. An OS
  recorder starting/stopping cannot take an existing hardware clock's authority.
- The existing native renderer accepts a mixed device set. Same-clock bindings
  still map only to verified native buffer offsets; independent hardware keeps
  its explicit bridge. The pacing output renders in its IOProc instead of waiting
  for a polling graph thread plus a second output-transfer worker.
- Independent devices are retained by a separate `DeviceSet` for the renderer's
  full lifetime. The callback uses existing graph read/write/control paths, with
  the same single graph-consumer/producer roles. It never changes converter
  ownership or aliases another device's ABL. Registration drain precedes release.
- Queue targets account for actual prepared converter input/output batch sizes,
  not only 32-frame device/DSP blocks. Previously a full converted batch could
  remain in private staging while the deadline-facing queue emptied before the
  worker woke. Equal-clock copies keep their smaller callback-derived target.
  This changes transport headroom, not the user's DSP buffer setting.
- Added control-side per-device starvation snapshots. For input they observe the
  graph-facing consumer; for output they observe the native consumer. Empty polls
  by conversion workers are deliberately not labeled missed audio deadlines.
  The new FFI accessor reads existing atomic counters; callbacks do not log.

The last change adds no RT allocation, logging, native property calls, cursor
sharing, or new unsafe ownership. Existing unsafe descriptor publication and
drain remain the lifetime boundary. Native queue metrics have a documented
retained-owner contract and a null-safe read-only regression.

## Focused verification

- Shared converter suite: 14 passed, including actual converter quantum and
  prepared-path allocation checks.
- Synthetic native device suite: 16 passed, including clock priority, OFF/ON,
  ownership retirement, and complete split-callback transfer.
- New 10-second synthetic 96 kHz / 32 native -> 48 kHz / 32 graph scheduling case:
  worker wakes once per three native callbacks (1 ms); input and output both
  contain 0 incorrect PCM frames after priming. It does not inject padding or
  discard measured short reads.
- Existing 60-second drift cases at -300 / 0 / +300 ppm: 0 missing/overflow frames,
  peak source backlogs 2,432 / 2,432 / 2,560; final source backlogs 0; correction
  +310.995 / +2.407 / -311.434 ppm. Not an end-to-end latency measurement.
- Final macOS execution/boundary suite: 81 passed; direct registration/metrics: 6
  passed; native device I/O/FFI: 13 passed, including the starvation accessor.
- Strict Clippy (`--all-targets -- -D warnings`) passed for engine-service,
  audio-io and macOS transport. Engine build, documentation for those three
  packages, `cargo fmt --all --check`, and `git diff --check` passed.

No full workspace or dedicated external-hardware fixture pass is claimed.

## Live per-device follow-up

The telemetry-only engine was rebuilt and loaded through an app-only restart at
02:25 UTC. Both old desktop/engine processes exited before reopening the same
bundle. Microphone and routed output meters remained nonzero after restoration;
processing stayed at 48 kHz / 32 frames and graph-1 revision became 692.

From 02:25:03.852 to 02:25:22.181 UTC (18.329 seconds), the deadline-facing ring
starvation counters changed as follows:

| Device / consumer | Start | End | Increment |
| --- | ---: | ---: | ---: |
| 96 kHz microphone / graph input (owner 7) | 0 | 78 | 78 |
| 96 kHz headphones / native output (owner 4) | 3 | 55 | 52 |
| 48 kHz speakers / native output ring (owner 1) | 0 | 0 | 0 |

These are transitions from a primed ring into shortage, not a count of perceived
clicks or every missing sample/block. The speakers render directly rather than
consume that ring: its zero counter does not exclude upstream microphone gaps.
Native clock-gap counters stayed zero. Microphone/output overflow counters also
rose to 21/13, with bridge epochs changing from 12/11 to 15/16. Thus queue loss and
bridge recovery remain present even without a native timestamp discontinuity.

This is not only a startup observation: in the preceding same-topology run,
at 02:21:07 UTC (about seven minutes after launch), microphone/headphone overflow
totals were 514/45 with bridge epochs 33/34. The earlier 30-second zero-run result
does not establish sustained continuity. The latest per-device observation is
telemetry, not another recording or a new accepted performance comparison.

## Remaining boundary

Independent hardware conversion still runs on ordinary polling workers, outside
the device callback's scheduling deadline. The native renderer's measured work
in observed 5-second windows was below its deadline, but this does not prove the
separate conversion workers met theirs. Per-device starvation now confirms
misses at both the microphone graph consumer and headphone native consumer;
that ownership/scheduling boundary still requires repair and renewed measurement.
Do not enlarge a fixed FIFO, suppress errors, or claim waveform continuity solely
because a filtered recording has no exact zeros. A simultaneous native source /
destination capture and headphone-loopback measurement remain required for true
end-to-end latency and final audible acceptance.
