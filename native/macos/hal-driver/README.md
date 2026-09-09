# ToneNari macOS HAL

The `AudioServerPlugIn` bundle is built from `tonenari-hal-core` (Rust) and
`driver/ToneNariHALBridge.c` (Apple COM and dispatch-block adapter). Device
registry, properties, format changes, clock arithmetic, mapping ownership and
PCM processing belong to Rust. The shared transport implementation is in
`native/macos/common`; it is linked into both the HAL and engine.

## Build and verification

```sh
cargo run -p tonenari-hal-bundle -- --output target/macos-hal
cargo test -p tonenari-hal-core -p tonenari-macos-transport
cargo test -p tonenari-engine-service --bin tonenari-engine-service native_callback_and_hal_contracts -- --exact --nocapture
```

The Cargo-owned builder compiles arm64 and x86_64 release archives, links only
the current ABI bridge, and validates the two architecture slices, factory
export, plist and signature. Its factory/COM probe runs on the host architecture
without initializing HAL or contacting the installed engine. The other slice
has compile/link/export validation, not runtime verification. Building does not
install the driver or restart CoreAudio.

The engine test executes native callback, device, HAL dispatch, application
clock and audio-ring fixtures. Native fixture code is built with ASan/UBSan;
this does not imply sanitizer instrumentation of the linked Rust archive.
Rust transport and lifecycle tests execute the current implementation directly.
Device-free tests do not prove live audio continuity or physical-clock behavior.

## Runtime boundaries

- Each virtual device has its own identity, name, channel geometry and native
  sample rate. The registry and inventory protocol support multiple devices;
  graph-level catalog and resource integration must be verified separately.
- HAL requests inventory over `io.tonenari.audio.engine`; the engine publishes
  anonymous descriptor-backed mappings. No filesystem transport path or shared
  singleton device ID is part of that contract.
- CoreAudio configuration changes use `RequestDeviceConfigurationChange`.
  Replacement mappings are validated before publication; retired resources are
  reclaimed only outside callbacks after readers release their pins.
- Each direction has one PCM writer. Absolute-frame tags, mapping incarnation
  and epochs reject stale PCM across restart, replacement and wraparound.
  Exhaustion requires a new mapping, never generation reuse in the same storage.
- Callback work must be bounded, allocation-free, lock-free and nonblocking.
  Shared-buffer capacity is not a target queue depth or fixed audio latency.
- A native endpoint owns its format. Conversion and drift handling belong only
  at actual independent clock boundaries, not to a global graph sample rate.

## Installation and acceptance

The development install entry point is `pnpm install:macos-dev-audio`. It changes
the local engine/HAL installation and can interrupt system audio; run it only
within the authorized installation workflow. Engine and HAL must use the same
current transport contract. Do not retain a previous implementation as a
fallback for an already-running process.

This product is unreleased. Replaced code, protocols, test implementations and
build paths are removed, not kept as compatibility modes or backup copies.
Required features and user graphs/settings must be preserved.

Current integration gaps, measured baselines and verification evidence live in
`docs/validation/2026-09-08-device-clock-routing.md` and
`docs/validation/2026-09-08-ownership-audit.md`. Paired installation, actual app
behavior, cross-graph sharing, and same-condition 32/256-sample audio acceptance
remain separate from a successful bundle build. Do not infer their completion
from unit tests or earlier installed-driver measurements.
