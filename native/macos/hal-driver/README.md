# MixRelay Driver — macOS

The `AudioServerPlugIn` bundle is built from `mixrelay-hal-core` (Rust) and
`driver/MixRelayHALBridge.c` (Apple COM and dispatch-block adapter). Device
registry, properties, format changes, clock arithmetic, mapping ownership and
PCM processing belong to Rust. The shared transport implementation is in
`native/macos/common`; it is linked into the HAL, independent Driver service and
graph's normal CoreAudio client adapter.

## Build and verification

```sh
cargo run -p mixrelay-hal-bundle -- --output target/macos-hal
node scripts/lab/native-check.mjs cargo test -p mixrelay-hal-core -p mixrelay-macos-transport
node scripts/lab/native-check.mjs cargo test -p mixrelay-engine-service --bin mixrelay-engine-service native_callback_and_hal_contracts -- --exact --nocapture
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

The existing `MRInstalledEndpointInspect` fixture accepts the exact canonical
CoreAudio UID. UID-only invocation reads the installed device and its native
master controls. Optional `--output-volume SCALAR` (finite 0–1) and
`--output-mute 0|1` set only that device and return native readbacks; all supplied
arguments are validated before any control write. Readback is not PCM proof.
`--render-format` and `--capture-format` accept `float32`, `signed16`, `signed24`,
or `signed32` on the exact selected UID, preserving its rate and width. They
validate native support before writing and wait for physical format readback.
`--driver` instead reads the installed plugin ID and its device IDs without
changing audio state; plugin ID `0` means the bundle is not loaded in CoreAudio.

`pnpm smoke:macos-installed-duplex <canonical-uid> <expected-gain>` checks actual
2/2-channel, 48 kHz PCM at gain `1`, `0.5`, or muted `0`. It changes no device
settings or graphs and uses the actual directional native ASBDs. Positive levels
allow 10 ppm relative SRC rounding error plus one native integer LSB;
mute requires exact zero. This checks amplitude and mute, not latency or continuity.

`pnpm probe:macos-roundtrip OUTPUT_UID [INPUT_UID]` measures a path on installed
devices of any vendor, so MixRelay, Loopback and BlackHole can be compared on one
Mac. It plays a deterministic signal into the output device and records the input
device (default: the same device). Signal samples are exact in Float32 and every
integer depth, never zero, carry independent content per channel (a mix of
channels is detected), and encode their channel and frame position, so the probe
tracks every signal frame in linear time: received, missing (head and tail separately) and repeated frames,
damaged samples (zero, wrong, non-finite), slips, channel order and channel
changes. Latency is measured in host time (with each timestamp's rate scalar) and,
on one device, also in device frames; the devices' reported latency (latency +
safety offset + stream latency, unknown when a query fails) is printed beside it.
`--list` prints device UIDs; `--json` prints one object; `--self-test` checks the
analysis on synthetic damage without devices. It changes no default device or
device setting. Recording needs microphone permission for the terminal that runs
it; without it every input is silent and the probe exits with status 3.

The installed Driver continuity test uses normal Rust `Native` clients and the
production native PCM codecs, with no graph server or session changes:

```sh
MIXRELAY_TEST_DRIVER_UID='io.mixrelay.audio.endpoint.<exact-32-lowercase-hex-id>' \
MIXRELAY_TEST_DRIVER_CHANNELS=32 MIXRELAY_TEST_DRIVER_FORMAT=signed24 \
node scripts/lab/native-check.mjs cargo test -p mixrelay-engine-service \
  --bin mixrelay-engine-service \
  macos_devices::live_tests::installed_driver::installed_driver_pcm_continuity_survives_other_client_detach \
  -- --exact --nocapture --test-threads=1
```

Replace the UID placeholder with the actual installed UID. Configure the exact
device through Driver control beforehand at 48 kHz, matching playback/capture
width `2`, `32`, or `64`, and native `float32`, `signed16`, `signed24`, or `signed32`.
Use native volume 1 and mute off; restore settings through the same Driver API
after the matrix. The test only reads actual directional ASBD tuples before
connecting. It skips exactly the first 48,000 captured frames as initial priming,
then requires at least 480,000 fresh indexed frames with every lane matching exact
PCM16-representable binary words in disjoint channel bands within `[-1, 1)`.
Missing, repeated, reordered, corrupt or short
PCM fails without resynchronization or a rounding tolerance. Native timestamp
discontinuities after initial priming and any queue overflow also fail. Both native queues
are handled inside their actual callbacks, using the preceding admitted input
quantum; the absent first preceding block is the sole queue admission exception.
An ordinary graph capture client detaches between two 240,000-frame observation
phases; the producer and observer remain connected until the post-detach phase
finishes. A 20-second timeout bounds the run before the 20-bit identity wraps.
Hook storage remains retained until native unregister drains, including failure.

## Runtime boundaries

- Each virtual device has its own identity, name, channel geometry and native
  sample rate. The registry and inventory protocol support multiple devices;
  graph-level catalog and resource integration must be verified separately.
- Each native stream offers Float32 and packed little-endian signed 16/24/32-bit
  PCM at six supported rates. Encodings are directional; nominal rate is shared.
  Integer ReadInput/WriteMix convert at the HAL boundary while transport and DSP
  remain Float32. Conversion scratch is prepared on control and retired after IO
  drainage; the Float32 callback path still uses direct transport.
- HAL requests inventory over `io.mixrelay.audio.driver`; the independent Driver service publishes
  anonymous descriptor-backed mappings. No filesystem transport path or shared
  singleton device ID is part of that contract.
- CoreAudio configuration changes use `RequestDeviceConfigurationChange`.
  Replacement mappings are validated before publication; retired resources are
  reclaimed only outside callbacks after readers release their pins.
- HAL owns both PCM writers and loops OS playback into capture. Graph clients
  read capture and contribute playback through CoreAudio, without writing these rings.
  Absolute-frame tags, mapping incarnation
  and epochs reject stale PCM across restart, replacement and wraparound.
  Exhaustion requires a new mapping, never generation reuse in the same storage.
- Each output device exposes native master volume and mute controls. Scalar gain
  is linear amplitude with a -96 dB reporting floor; mute writes exact silence.
  These controls scale playback and its loopback before publication. Graph-local
  gain controls only that graph's contribution; it does not replace native controls.
- Callback work must be bounded, allocation-free, lock-free and nonblocking.
  Shared-buffer capacity is not a target queue depth or fixed audio latency.
- A native endpoint owns its format. Conversion and drift handling belong only
  at actual independent clock boundaries, not to a global graph sample rate.

## Installation and acceptance

Driver and HAL ship with the editor and engine in the complete MixRelay installer
described in [macOS packaging](../../../packaging/macos/README.md); there is no
separate Driver installation. Installing or removing HAL requires OS approval
and a CoreAudio reload, not a Mac reboot.
The installer warns about the brief system-audio interruption and verifies actual
activation. Runtime virtual-device edits require neither reload nor reboot. Driver and
HAL use the same current inventory contract. Do not retain a previous implementation
as a fallback for an already-running process.

This product is unreleased. Replaced code, protocols, test implementations and
build paths are removed, not kept as compatibility modes or backup copies.
Required features and user graphs/settings must be preserved.

Agreed behavior, current integration gaps and verification evidence are in the
[product document](../../../docs/product.md). Paired installation, actual app
behavior, cross-graph sharing, and same-condition 32/256-sample audio acceptance
remain separate from a successful bundle build. Do not infer their completion
from unit tests or earlier installed-driver measurements.
