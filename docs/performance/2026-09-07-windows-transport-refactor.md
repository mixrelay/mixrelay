# Windows transport contract refactor — 2026-09-07

## Outcome and scope

The Windows LAB mapping still runs the same native graph/PCM integration after
separating its wire contract from Win32 ownership. This is a correctness and
maintainability change, **not** configurable Windows device support or a performance
improvement. The normal product capability gate stays unchanged.

- `windows_transport_contract.rs` owns validated dimensions, v2 request/descriptor,
  PCM region sizes, immutable header checks and peer-counter validation. It has no
  OS handle, pointer dereference, allocation or DSP dependency and is tested on macOS
  and Windows ARM64.
- `windows_transport.rs` owns Win32 handles, mapping lifetime, acquire/release
  publication and native copies. No new mapping/free API or default-device fallback
  was introduced. Event handles still close before the file releases mapped pages.
- Copies now use validated channel counts instead of separate stereo/mono literals.
  Four-byte sample addressing also preserves odd-capacity/odd-channel capture offsets;
  rounding those offsets to an eight-byte header word would be incorrect.
- Descriptor validation checks the entire range in the caller's address width
  before header dereference. Writable header offsets never control copy addresses.
- The 256-frame startup silence remains once per mapping, not once per graph revision.
- The UI fan-in test's outdated “rejects occupied inputs” name was corrected, and
  assertions now confirm adding a second source preserves the existing cable.

## Tests

| Boundary | Result |
| --- | --- |
| macOS engine-service library | 16 passed (11 existing + 5 contract tests); Clippy including tests passed |
| Windows ARM64 engine-service | Build and Clippy passed; 16 library + 10 binary tests passed; 2 older interactive tests remain ignored, not counted as passing |
| Contract dimensions | 50,688 combinations: every nonempty 0–64 render/capture pair × 4 capacities × 3 rates; disjoint bounded, page-rounded regions |
| Invalid contract input | Invalid dimensions/rates, every header-prefix field, each PCM extent field, descriptor ABI/extent/alignment/overflow rejected |
| Native copy fixture | Actual adapter copies checked with distinct per-channel binary fractions across wrap: 2→1, 64→16 and 1→3 at odd capacity 257; counter publication and zero-fill checked |
| Portable graph/audio-io/DSP/control-server | 51 tests passed, including fan-in, explicit routing, one-way endpoints, stale/typed control errors and allocation-free DSP boundary |
| UI | All 62 tests passed; TypeScript check passed |
| Lab harness | All 38 tests passed; generated literal command matches reviewed source; no security-policy changes |
| Patch hygiene | `git diff --check` passed; no commit, staging or branch changes |

These layout/copy tests do not prove native 64-channel endpoints. The actual installed
LAB driver pair remains stereo render → mono capture at 48 kHz.

## Same-condition native regression

Same Windows VM, installed driver, explicit render/capture IDs, probe v11, 48 kHz,
128-frame DSP block, 256-frame startup silence, channel-1 → −6.020599913 dB gain →
mono capture graph, three 3-second probe cycles and live label revision.

| Observation | Before: build/audio v9 | After: build v12 / audio v10 |
| --- | --- | --- |
| Capture matches, cycles 0/1/2 | 141442 / 142498 / 142018 | 140962 / 141538 / 142978 |
| Final observed native underruns / overruns | 2 / 0 | 2 / 0 |
| Native capture discontinuity flags per cycle | 1 / 1 / 1 | 1 / 1 / 1 |
| Startup lead | 256 frames / 5.33 ms | 256 frames / 5.33 ms |
| PCM / gain meter / live graph revision | pass | pass |
| Graph removal / mapping reattach | pass | pass |

After refactoring, 45 status samples covered ~9.17 seconds. Both observed underruns
appeared by 0.425 seconds; no later increase was observed. Final gain/output meter
peak was 0.12501526; submitted and active graph revision were both 3. This is one
bounded regression run, **not** throughput, latency or leak certification. The PCM
frame counts vary with native scheduling and are not an improvement metric. No
speedup is claimed or inferred from build times.

## Evidence, failures and cleanup

Evidence directory:
`/Volumes/Linear 1/test/mixrelay/evidence/windows-transport-refactor-20260907`.

- Build v10 compiled and passed tests, but failed Clippy on pointer alignment,
  narrowing casts and a float comparison. Its failure log is retained. Build v11
  passed after correction; v12 additionally uses distinct per-channel test signals.
- Actual PCM test: `C:\MixRelayLab\windows-rust-bridge-audio-v10`, exit code 0.
- Final engine SHA-256:
  `B4036C5326B48431C92D38C9124C7DC76CEB8F6E944D0013ADEF788E7A97FA56`.
- Owned interactive test task removed after completion. Final read-only inspection
  confirmed no owned task, engine or probe remains. Devices, source and all evidence
  retained. No reboot, driver replacement, signing change or host audio change.

Remaining implementation/qualification work is tracked in the
[product feature gates](../implementation-status.md). In particular, transport error
recovery, product endpoint configuration, missing hardware I/O, clock-domain handling,
distribution-specific audio and native plugin/editor qualification remain open.
