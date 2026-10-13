# MixRelay macOS application capture

This directory contains the Core Audio process-tap adapter, C ABI headers, and
fixtures used by `ApplicationCaptureBackend` on macOS 14.2 and later. The
`mixrelay-process-tap` Rust crate owns capture sessions behind that ABI; the
desktop capture pump delivers their PCM to application sources in the graph.

The data path is deliberately separate from the virtual endpoint driver:

1. Resolve a bundle ID selector to current Core Audio process objects. Bundle
   IDs are the only supported selectors; exclusion selectors are not implemented.
2. Create a private `CATapDescription` with the requested playback policy.
3. Create the process tap and attach its UID to a private aggregate device.
4. Run the aggregate device IO callback into a bounded, preallocated ring
   buffer.
5. Wake the desktop capture pump through tap callback notifications, convert
   captured PCM, and publish it into a separate application-source shared-memory
   transport obtained from `io.mixrelay.audio.engine`.
6. Consume that transport in the graph application node's `audio` output.

Capture is used by the graph as well as the diagnostic probe. Process launches,
exits, helper-process churn, default-output changes, sleep/wake, and permission
changes remain lifecycle acceptance cases; the persisted selector is a bundle ID.

The native tap, aggregate device, IOProc, and ring buffer are owned by one
session. Rust wraps that session with RAII, so dropping it stops capture and
releases the native resources in reverse creation order. Partial construction
failures use the same native cleanup path.

The app must provide `NSAudioCaptureUsageDescription`. Permission denial is not
yet exposed as a distinct typed backend state. Adding that state, rather than
reporting denial as “application unavailable,” remains an open requirement.

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

Run Cargo verification from the repository root through the supported wrapper,
which prepares the HOST Cargo static libraries needed by native fixtures:

```sh
node scripts/lab/native-check.mjs cargo test -p mixrelay-process-tap -p mixrelay-macos-transport
node scripts/lab/native-check.mjs cargo test -p mixrelay-engine-service --bin mixrelay-engine-service native_callback_and_hal_contracts -- --exact --nocapture
```

The engine contract test runs the native callback, HAL, application-clock, and
bounded ring fixtures compiled with strict warnings, AddressSanitizer, and
UndefinedBehaviorSanitizer. The linked Rust archives are not sanitizer-instrumented.

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

Tauri exposes discovery and probe diagnostics to the UI. Raw PCM stays inside
the native/Rust audio boundary and reaches the graph through application-source
shared memory; it does not cross the webview bridge.

Apple's sample “Capturing system audio with Core Audio taps” is the behavioral
reference. MixRelay implements the backend independently and keeps Core Audio
types out of the shared Rust contracts.
