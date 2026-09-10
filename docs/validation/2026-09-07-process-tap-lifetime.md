# macOS process-tap lifetime and format invalidation

## Scope and evidence

The production implementation in `native/macos/application-capture/src/MRProcessTap.m`
is compiled directly into `native/macos/application-capture/tests/MRProcessTapLifetimeTests.m`.
Only Core Audio object operations are substituted with failure-injection functions.
The test uses the production Rust audio ring and actual libdispatch cleanup queue/retry.
It does **not** create a real tap/device, request capture permissions, change the
default device, or modify the running engine/session.

Result: strict compilation and AddressSanitizer/UndefinedBehaviorSanitizer run
passed. Output:

```text
PASS: 1000 teardown failure/recovery cycles; format/rate invalidation; malformed ABL; bounded async destruction (2014 unregister calls)
```

## Reproduce on macOS

Requires the installed Command Line Tools/macOS SDK with process-tap APIs
(deployment target macOS 14.2) and the repository's Rust toolchain. Cargo's
`macos-test-support` build owns the native executable and links the current Rust
callback-clock and capture-ring implementations. The local compiler resolves the SDK through its
normal toolchain. Foundation, CoreAudio, and AppKit are required because the
production translation unit includes application discovery/icon code as well as
the tested capture lifecycle.

From the repository root:

```sh
cargo test -p mixrelay-engine-service --bin mixrelay-engine-service \
  native_callback_and_hal_contracts -- --nocapture
```

This selection also runs the callback-clock, device, HAL, application-clock and
audio-ring fixtures. The Objective-C fixture retains AddressSanitizer and
UndefinedBehaviorSanitizer; the linked stable-Rust archive is not sanitizer
instrumented. All artifacts remain in Cargo's output directory. The standalone
C-only link recipe was removed when the callback-clock algorithm moved to Rust.

Compile the production Objective-C translation unit independently:

```sh
clang -std=c17 -fobjc-arc -fmodules -fblocks \
  -mmacosx-version-min=14.2 \
  -Wall -Wextra -Wpedantic -Wconversion -Wsign-conversion -Wshadow -Werror \
  -fsyntax-only -I native/macos/application-capture/include \
  native/macos/application-capture/src/MRProcessTap.m
```

## Contracts exercised

- Stop failure preserves the running ownership flag. Unregister failure retains
  the IOProc identifier, callback context, and ring. Listener/aggregate/tap
  destruction failures each retain their corresponding owned resource.
- Reclamation waits for successful IOProc/listener removal and zero in-flight
  callback/listener counters. A later successful cleanup releases the ring.
- Destroy transfers ownership to a serial cleanup queue without waiting for a
  native call. A deliberately blocked unregister does not block the destroy
  caller. A transient failure is retried by the real dispatch timer, and its
  admission slot is released after successful cleanup.
- A tap-format listener invalidates delivery. A serialized control-owner refresh
  unregisters IO before draining the SPSC ring. Same-layout 48 kHz → 44.1 kHz and
  same-rate discontinuities increment `format_generation`. Ring allocation and
  channel geometry remain immutable for concurrent meter/stat readers.
- A listener arriving during a format query is not acknowledged by an older
  snapshot. Refresh returns `RESOURCE_BUSY` and a later refresh recovers.
- Incompatible layout and malformed callback buffer geometry do not enter the
  ring. A format-query error gates delivery. Starting an unregistered/gated
  session is rejected instead of passing a null IOProc to Core Audio.
- Ring overflow clock-credit check: 256 accepted + 256 capacity-rejected frames
  produce 512 total valid native frames. Null data, wrong channel geometry, and
  listener-gated callbacks add no credit. Refresh drains the ring without resetting
  lifetime counters; the next 256 valid frames raise the total to 768. This proves
  the native `captured_frames + dropped_frames` accounting at this boundary.
  These are separate atomic stat loads, not a transactional timestamp snapshot;
  a concurrent read can temporarily under-observe an in-flight callback.

## Remaining limits

This is a **native failure-injection fixture**, not live Core Audio teardown,
hotplug, default-device-switch, or live sample-rate-switch acceptance. It does
not validate Rust pump/SRC integration; that belongs to the separate app-capture
integration checks.

Permanently failing or hung native destruction retains owned resources rather
than risking use-after-free. At most 32 sessions can own admission; further
creation returns `RESOURCE_BUSY`. A permanently blocked Core Audio call can
stall the serial cleanup worker. This is bounded retained ownership, **not** a
claim of leak-free recovery from arbitrary native hangs. Process termination
remains the ultimate reclamation boundary for such failures.

Refresh `RESOURCE_BUSY` is transient and must be retried by the control owner;
`running` may be false during this suspension. Incompatible geometry requires
session recreation. Source conversion must discard its external FIFO and reset
on a new generation. Rate changes intentionally discard old buffered audio;
zero-dropout switching has not been established.

No throughput/performance improvement is claimed. The numeric counts above
measure lifecycle/failure coverage, not latency or CPU performance.
