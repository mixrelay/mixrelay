# MixRelay macOS application capture

This directory contains the first native macOS implementation slice of
`ApplicationCaptureBackend`. It uses Core Audio process taps on macOS 14.2 and
later and exposes a stable C ABI to the Rust desktop wrapper.

The data path is deliberately separate from the virtual endpoint driver:

1. Resolve a bundle ID or exclusion selector to current Core Audio process
   objects.
2. Create a private `CATapDescription` with the requested playback policy.
3. Create the process tap and attach its UID to a private aggregate device.
4. Run the aggregate device IO callback into a bounded, preallocated ring
   buffer.
5. Read captured PCM through the bounded ring-buffer consumer.

Steps 1 through 5 are implemented by the capture probe. Connecting the consumer
directly to the graph node's `audio` output port, and reconciling process
launches, exits, helper processes, default-output changes, sleep/wake, and
permission changes without changing the persisted selector, remain engine work.

The native tap, aggregate device, IOProc, and ring buffer are owned by one
session. Rust wraps that session with RAII, so dropping it stops capture and
releases the native resources in reverse creation order. Partial construction
failures use the same native cleanup path.

The app must provide `NSAudioCaptureUsageDescription`. Permission denial is a
typed backend state and must not be flattened into “application unavailable.”

Implementation constraints:

- no allocation, locking, logging, or object discovery in the IO callback;
- never use localized application names as selectors;
- preserve source playback unless `MuteWhileCaptured` was requested;
- destroy IOProc, aggregate device, and process tap in reverse creation order;
- generation-tag samples so stale callbacks cannot enter a replacement source;
- test Electron/browser helper-process churn and game launcher-to-child process
  transitions;
- test default output changes, device removal, sleep/wake, permission revocation,
  and long-running capture;
- treat a zero-filled buffer as valid silence unless another health signal proves
  the tap failed.

## Local verification

The default repository gate compiles the native boundary with strict warnings
and runs the bounded SPSC ring-buffer unit tests under AddressSanitizer and
UndefinedBehaviorSanitizer:

```sh
pnpm check:native
```

The opt-in smoke test builds a temporary menu-bar-only AVAudioEngine tone source,
captures it through the real process-tap path, and rejects silence, missing
callbacks, dropped frames, or invalid stream metadata:

```sh
./scripts/smoke-macos-application-capture.sh
```

On 2026-09-02 the repeated local run captured 114,688 stereo frames at 48 kHz
across 224 callbacks, reported a peak of 0.059986, and dropped zero frames. This
proves the native source boundary and bounded PCM transfer; it does not yet prove
graph routing, virtual-device delivery, process restart recovery, or long-run
leak freedom. The installed macOS sanitizer runtime does not support leak
detection, so lifetime validation still needs Instruments and soak testing.

The Tauri commands currently expose discovery and probe diagnostics only. Raw
PCM intentionally does not cross the webview bridge; the engine will consume it
inside the native/Rust audio boundary.

Apple's sample “Capturing system audio with Core Audio taps” is the behavioral
reference. MixRelay implements the backend independently and keeps Core Audio
types out of the shared Rust contracts.
