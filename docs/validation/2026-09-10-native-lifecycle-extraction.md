# Native lifecycle extraction

Scope: macOS-first cleanup, not Windows/Linux port implementation or installed
audio-device acceptance. Existing graph data, running processes and the Git index
are not changed by this validation.

## Ownership boundaries

- Device registration: `device_io_lifecycle` owns the procedure/device pair and
  create/start failure cleanup. Native adapters collect device facts and call
  CoreAudio. An unconfirmed unregister retains the exact registration and its
  outer callback context; PCM queues and callback-local cursors are separate.
- Process tap: `process_tap_lifecycle` owns bounded admission, format invalidation
  and validation epochs, and the one-time retirement gate. Native resource
  unregister, callback/listener drain and asynchronous retry orchestration still
  remain in Objective-C. Admission returns only after final native cleanup.
- HAL control: `control_session` owns connection authority transitions and the
  ten-tick heartbeat. XPC/dispatch handles and Blocks remain in the native adapter;
  existing registry inventory reconciliation remains its sole policy owner.
  No mutable control-session reference crosses a registry/host callback.

No replacement fallback, extra worker, heap allocation in a PCM callback, new
platform adapter or manual `Send`/`Sync` implementation is introduced.
Tap property notifications and admission use atomic CAS retries on control/listener
paths. The PCM delivery gate uses only atomic reads; this is not a claim that
every native property listener is wait-free.

## Validation

Final frozen-source selection: **412 tests passed, zero failed or ignored**.

| Selection | Passed |
| --- | ---: |
| macOS transport (including 14 new device/tap lifecycle tests) | 124 |
| HAL core (including 6 new control-session tests) | 44 |
| Publication models | 2 |
| Native C/Objective-C contracts | 1 |
| Engine device boundaries | 13 |
| Desktop clock pump | 4 |
| Engine library | 214 |
| PCM ring | 10 |

Native contracts include partial-create/start/unregister failure ownership,
queue/timer/socket allocation failure, stale connection events, 1,000 tap
teardown/recovery cycles, and retained duplicate-destroy handling. The device
stress test completed 512 create/retire cycles with zero surviving owners and
`leaks` reporting zero leaked bytes. C/Objective-C fixtures use ASan/UBSan; Rust
objects linked into them are not sanitizer-instrumented.

The queue/timer failure case was found during this batch: a missing serial queue
could otherwise send XPC callbacks to a concurrent default queue. Initialization
now requires both dispatch resources before publishing callbacks and releases
partial resources/core on failure. The final native fixture enters the actual
initialization function for both injected allocation failures.

Executed successfully on the final source:

```sh
cargo clippy -p mixrelay-macos-transport -p mixrelay-hal-core -p mixrelay-macos-test-support -p mixrelay-engine-service -p mixrelay-desktop --all-targets -- -D warnings
cargo test -p mixrelay-macos-transport -p mixrelay-hal-core --quiet
bash scripts/check-macos-clock-boundaries.sh
cargo test -p mixrelay-engine-service --lib --quiet
cargo test -p mixrelay-pcm-ring --quiet
cargo build -p mixrelay-engine-service -p mixrelay-desktop
cargo run -p mixrelay-hal-bundle -- --output target/test-artifacts/lifecycle-hal
cargo doc -p mixrelay-macos-transport -p mixrelay-hal-core --no-deps
cargo fmt --all --check
git diff --check
```

The actual HAL bundle was built for arm64 and x86_64, checked for its single
factory export, ad-hoc signed and signature-verified. Its factory/COM probe ran
on the local Apple Silicon host without Initialize/StartIO. Intel execution,
installed HAL behavior, physical hotplug and live playback were not exercised.
The desktop/engine binaries were built but not launched or installed.

Independent read-only review ran during implementation and after the final
queue/timer fix; no remaining blocking findings were reported for this slice.
This is not completion of the remaining native cleanup or cross-platform gates.

## Performance evidence

This is an ownership refactor, not an overall speedup claim. DeviceIO's PCM
callback body, queue algorithm and realtime call count are unchanged. Its new
registration adapters run on control; HAL reconnect/heartbeat also runs on
control. Native start/stop latency was not compared on installed devices because
this batch deliberately does not open or interrupt user audio. Failure-injection
tests instead check native-call order, retained handles and retry outcomes.

The tap delivery gate changes an actual callback path, so the unchanged
`MRProcessTapInputBench.m` harness was built before and after replacement.
Both use Clang `-O3 -fobjc-arc -fblocks -mmacosx-version-min=14.2`, the Rust release
static library, no extra `RUSTFLAGS`, capacity 16,384, 20,000 iterations per pass,
one warmup and seven measured passes with bit-exact PCM assertions. Executables
ran sequentially with all builds/tests idle. Units below are median nanoseconds
per callback plus ring drain; negative percentages mean less elapsed time.

| Channels | Frames | Layout | Before | After | Change |
| ---: | ---: | --- | ---: | ---: | ---: |
| 2 | 32 | packed | 87.90 | 80.95 | -7.9% |
| 2 | 32 | planar | 84.00 | 80.35 | -4.3% |
| 2 | 256 | packed | 578.85 | 572.85 | -1.0% |
| 2 | 256 | planar | 534.65 | 565.95 | +5.9% |
| 8 | 32 | packed | 235.65 | 238.25 | +1.1% |
| 8 | 32 | planar | 249.80 | 253.65 | +1.5% |
| 8 | 256 | packed | 1802.45 | 1829.00 | +1.5% |
| 8 | 256 | planar | 1934.90 | 1955.10 | +1.0% |
| 64 | 32 | packed | 1744.15 | 1681.55 | -3.6% |
| 64 | 32 | planar | 1949.30 | 1858.65 | -4.7% |
| 64 | 256 | packed | 12584.80 | 14241.20 | +13.2% |
| 64 | 256 | planar | 15074.55 | 15031.00 | -0.3% |

Run-to-run variation is material: the preceding matched run measured 64-channel
packed/256 at 14080.20 -> 12671.05 ns, the opposite direction. Stereo planar/256
was slower in both runs (526.80 -> 567.00 ns in the preceding run). The new gate
adds an FFI boundary and rejects exhausted epochs; these measurements do not
establish either a universal speedup or no regression. No PCM algorithm or
validation guarantee was removed to improve the table.

Raw logs and the pre-replacement executable are ignored build artifacts under
`target/test-artifacts/tap-lifecycle-benchmark/`; the table uses
`before-verified.txt` and `after-verified.txt`. No old production source is retained.
