# Nested menus and macOS driver presence

## Changes and evidence

- Nested menus stay in their parent DOM for outside-click and focus handling.
  Fixed-position descendants can extend beyond the parent's scroll viewport.
  Root menus remain portalled. No global dismissal handler was disabled.
- Regression: the four new nested context/button mouse/keyboard cases failed before
  the fix; all seven menu tests passed afterward. Keyboard unit coverage navigates
  with arrows; leaf activation is clicked in jsdom. Native keyboard Enter is verified below.
- Actual rebuilt MixRelay app: source menu by mouse and output menu by keyboard
  both reached New device. Created distinct named input/output test devices and
  confirmed each in `system_profiler SPAudioDataType` as real 2-in/2-out devices.
  Cmd-Z removed each test creation; the original microphone-to-MixRelay connections
  remained. No unrelated device or graph was removed.
- macOS driver presence now follows authenticated HAL inventory subscriptions in
  the existing off-RT monitor, including when no virtual devices are referenced.
  XPC disconnect removes the peer. The query does not create a device or infer
  readiness from audio callback activity. It adds no C or realtime allocation/lock.
- Restored missing `endpoint_observation_tests.rs` referenced by the previous work:
  inactive-graph observations, single read per shared identity, metadata preservation,
  invalid/missing observations, and no native setter/resource preparation.

## Validation

- Frontend focused regression: 55 passed across four files, including existing
  one-sided-device and hidden-cable deletion regressions. UI boundary check passed.
- Engine library: 176 passed, zero failed/ignored.
- Changed worker tests: 7 passed; direct-owner lifetime test: 1 passed.
  These focused binary tests do not constitute a full native-device suite run.
- Engine all-target strict Clippy passed after fixing unused ownership plumbing,
  a redundant closure, test initialization, and exact bitwise float assertions.
- UI TypeScript/Vite build, desktop debug app bundle, engine build, focused Biome,
  and git diff whitespace checks passed. Vite retains its large-chunk warning.
- Independent read-only reviewer found no new P1/P2 in the menu/presence changes.
- Engine-only restart and actual Ready status verification are pending administrator
  authentication. No CoreAudio restart or HAL replacement is required by this change.

These are correctness fixes, not a latency or throughput optimization claim.
The 4-fail to 7-pass menu comparison and actual native interaction are the evidence;
no performance benchmark or whole-product completion is claimed.
