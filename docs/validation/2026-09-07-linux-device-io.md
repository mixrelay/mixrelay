# Linux device I/O parity slice — 2026-09-07

## Implemented boundary

Linux previously enumerated devices but rejected both `audio_device_input` and
`audio_device_output` in its engine capabilities. Both now have real PipeWire
streams wired to graph external I/O, not simulated meters or capability-only flags.
The public `DeviceSelector` lives in `audio-io`, re-exported from `audio-graph`
without changing existing JSON. Its direction, exact identity and 1–64-channel
validation are shared contracts; this does **not** claim every OS now implements I/O.

Linux resolves persistent `node.name` selection to this session's `object.serial`.
Missing, ambiguous, wrong-direction and undersized devices reject the edit without
replacing the active graph. Unknown channel width is zero, not guessed stereo.
Only actual Node registry objects become audio targets; Client objects may supply
application identity, but cannot duplicate nodes or supply their routing identity.
Numeric and string PipeWire properties are both handled. Old session-serial device
selections are not silently remapped to another device.
Final metadata regression also keeps hardware display names and channel widths on
the Node: the owning client name (for example WirePlumber) cannot replace them.

Native target streams disable remix, reconnect and default-device fallback per
[PipeWire's property contract](https://docs.pipewire.org/page_man_pipewire-props_7.html).
Opening waits at most two seconds for STREAMING, not merely registered/PAUSED.
Native creation, failed preparation and teardown run off the DSP thread. Prepared
device sets swap at the same boundary as the graph, and retired owners are drained
by the existing reclaimer. Application captures and virtual endpoints retain their
separate ownership. Gain changes stay in DSP and do not reopen device streams.
Device loss transfers silence/discontinuity and leaves the graph clock/control API
running; automatic reopening after device arrival is still unfinished.

## Native Ubuntu ARM64 acceptance

`scripts/lab/linux-device-probe.mjs` owns a fresh engine, configuration directory
and two explicitly named fixture devices. It refuses an occupied control port and
does not change default devices. `MRDeviceFixture.c` produces/measures PCM directly
in independent PipeWire callbacks, outside MixRelay's transport rings/meters.

Final run: `device-native-v7`, exit **0**.

| Actual native route | Active frames measured | Longest internal silent run |
| --- | ---: | ---: |
| 2 → 2, reversed | 77,648 | 1 frame |
| 8 → 8, reversed | 78,261 | 1 frame |
| 16 → 16, reversed | 77,157 | 1 frame |
| 32 → 32, reversed | 78,016 | 1 frame |
| 64 → 64, reversed | 78,139 | 1 frame |
| 64 → 16, channels 13 → 1 and 33 → 16, live gain edit | 78,507 | 1 frame |
| 8 → 2, channel summation | 78,139 | 1 frame |

All per-channel peaks match expected values within 0.00002; all samples are finite.
The one-frame silent runs are the generated sine's zero crossings, below the
48-frame gate. With −6.0206 dB input gain, output channels 1/16 measure
0.006347656 / 0.016113281; all unconnected output channels remain zero. Both input
and output meter widths/values match PCM. Native stream serials remain unchanged
across gain edits. Removing fixture devices clears meters to zero without stopping
the engine. Recreating devices under the same names resolves their new serials.

Missing identity, wrong direction and excessive output width are rejected with no
partial streams and no graph revision change. Twelve 64/16 create/remove cycles
leave no owned device streams. Post-warmup RSS is 88,864–88,868 KiB (4 KiB range);
all engine/fixture nodes disappear after clean process exit. This is bounded short
run evidence, **not a universal leak-free or endurance guarantee**. No speedup is
claimed: the previous Linux device path was unsupported, so a like-for-like PCM
performance comparison is unavailable.

The existing PCM ring suite also passes AddressSanitizer/UndefinedBehaviorSanitizer
for native quantum splitting, channel identity, wrap, overflow, silence, recovery
and concurrent ownership. Sanitizer coverage here is the ring, not the whole engine.

## Regression checks and honest limits

- macOS: audio-io 14 + graph 15 + engine 25 = **54 tests passed**; affected shared
  crates pass Clippy. No running macOS service/driver/default-device changes.
- Ubuntu: audio-io 18 + graph 15 + engine 27 = **60 tests passed**, Clippy passed.
- Windows ARM64 VM: **68 tests passed**, **2 interactive WASAPI tests ignored**;
  Clippy passed. This is shared-contract/build regression, not new Windows PCM proof.
- Rust formatting and tracked diff whitespace checks pass.

Earlier failed runs are retained: v1 had an outdated guest helper; v2's fixture
treated shutdown EINTR as failure and its cleanup assertion masked the original
failure; v3 correctly refused the leftover test engine. That engine was identified
by exact executable and isolated configuration before targeted SIGTERM. v4 exposed
Client objects being misclassified as duplicate devices. Cleanup now attempts both
owners even if one fails, the parser regression includes the Client case, and v5–v7
native routing passes. v6 added explicit meter/loss assertions; v7 added continuity.

Not qualified: physical 64-channel ADC/DAC hardware, independent-device clock drift,
other rates/quanta, automatic device-return recovery, long endurance, Debian/Kali/
Arch native sessions, macOS hardware I/O or Windows hardware input. AU/VST/editor
acceptance and Windows product driver deployment remain separate unfinished gates.
Feature parity remains the product requirement, not the current completion state.

Evidence: `/Volumes/Linear 1/test/mixrelay/evidence/linux-device-20260907-v7`
(`result.json`, `native.log`, `meters-64-16.json`, `linux-tests-clippy.log`,
`windows-tests-clippy.log`). Earlier run logs remain in Ubuntu `/var/tmp`.
The final metadata-only follow-up passes the same 60 Linux tests and Clippy
(`final-linux-tests-clippy.log`). Native v7 measurements above precede that follow-up.
