# macOS initial HAL attachment request recovery

## Confirmed failure

CoreAudio rejected the initial `RequestDeviceConfigurationChange` before publishing
the device (`can't find a device/endpoint with that ID`, 00:20:31.387 KST;
publication followed at 00:20:31.391). The driver ignored the returned status,
leaving the pending shared-memory descriptor unattached indefinitely. This is
separate from the running-transport clock reset fixed in the preceding slice.

After the user shut down the VMs, the unchanged installed duplex test still
reported `compared=281600 matching=0 nonzero=0 invalid_callbacks=0`, exit 6.
This establishes that removing VM pressure did not resolve the functional failure;
it is not a latency or CPU benchmark.

## Implementation

- A single coalescing control-plane task retries rejected requests with exponential
  backoff from 100 ms to at most five seconds. Recovery remains possible during a
  prolonged host startup; it does not silently expire after an arbitrary timeout.
- One pending descriptor, one scheduled task, and at most one host request are
  retained. Replacement closes the superseded descriptor. No retry work, locks,
  timers or allocation were added to the real-time callback.
- Host calls run outside the state mutex and carry an opaque generation token.
  Old Perform/Abort callbacks cannot consume or discard a newer transport. A
  replacement received during a host call is scheduled only after that call ends.
- A rearmable dispatch source also watches accepted requests. If neither host
  callback arrives within five seconds, it requests the same generation again.
  New attachments rearm that one timer immediately, without waiting for the old
  watchdog or accumulating cancelled blocks. Duplicate completed Perform calls
  are no-ops. Abort consumes the pending descriptor and disarms the timer.

## Deterministic verification

The fixture includes the actual driver translation unit, replacing XPC send and
dispatch/host boundaries. Before the fix it exited 134 because a rejected request
queued no retry. After the fix it passed 1,000 repeated rejections, 1,000 coalesced
replacements, stale Perform/Abort callbacks, replacement during a host request,
and cancellation of a queued retry, alongside the previous lifecycle regressions.

`zsh scripts/check-macos-native.sh`: passed, including ASan/UBSan, existing isolated
library 100% line/function coverage gates, and arm64/x86_64 HAL builds. Those
coverage percentages do not describe the entire embedded HAL translation unit.

## Installed verification

Installation completed with administrator approval. Built and installed HAL
executables both hash to
`8e990be1bb20df86c0e1dd2b26898de5a246efdb218a62aeb0f00d5daa5363d1`.
The service (PID 40510) responded with the saved 48 kHz / 256-frame processing
format restored, but still reported `clock_not_synchronized` and timer authority.
The repeated installed duplex test **still failed** with
`compared=281600 matching=0 nonzero=0 invalid_callbacks=0`, exit 6. This fix alone
has therefore not resolved the installed audio path; no PCM recovery is claimed.

CoreAudio logged the initial bad-device request again at 00:44:17.714 KST. That
single host log does not establish whether the subsequent retry was accepted,
aborted, or attached a transport that later became obsolete. Those boundaries
need further instrumentation before attributing the remaining failure.

Tauri dev was restarted with `pnpm desktop:dev` (frontend 1420, desktop PID 40824).
Native accessibility inspection was denied, so a visual inspection is not claimed.
Current user graph and preferences were backed up at
`/tmp/mixrelay-macos-attach.2b8hUC`; no VM was started.

## Completion-watchdog correction and actual PCM recovery

The second correction replaces acknowledgement-only completion with a five-second
callback watchdog, plus idempotent duplicate Perform handling. The fixture now
also exercises success-with-no-callback, duplicate completion without epoch reset,
one timer across 1,000 replacements, and closed superseded descriptors.
The full native sanitizer/coverage/universal-build workflow passed again.

The subsequent installed helper PID 42779 provided direct evidence:

- 00:50:44.321 KST: generation 1 request returned **0** (success), but no Perform.
- 00:50:49.328: watchdog repeated generation 1; request again returned 0.
- 00:50:49.333: actual Perform succeeded, clock epoch 1 published.

The same installed CoreAudio duplex command then **passed**:
`compared=281600 matching=281600 nonzero=281600 invalid_callbacks=0`.
Before this correction the same test had 0 matching/nonzero samples. This verifies
sample identity recovery, not a CPU/latency improvement or long-run qualification.
The API reported 48 kHz / 256, device clock authority, and `issue:null`; after
the fixture ended it correctly returned to idle. Its cumulative underruns were
5 (overruns 0), so no zero-underrun endurance claim is made.

VMs stayed off. Sequential native device recovery/SRC checks then passed: actual
native input/output stayed at 48 kHz while graph rates changed 44.1 -> 96 -> 48 kHz,
with 95,744 native frames per phase and input/output FIFO overflow counts both 0.
These used owned temporary aggregates, removed after completion, not a physical
microphone or changes to host default devices.
