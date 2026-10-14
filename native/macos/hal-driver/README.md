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

## Transport-type compatibility

`kAudioDevicePropertyTransportType` (`tran`) normally returns `virt`. The Driver
service ships an exact, case-sensitive bundle allowlist derived from observed
Rogue Amoeba Loopback/ARK field behavior for apps that hide/reject virtual devices:
`com.tencent.tencentmeeting`, `net.whatsapp.WhatsApp`, `com.motu.DigitalPerformer`,
`com.vmware.fusion`, `com.goodnotesapp.x`. Only those requesters receive `usb `.
This changes a four-byte presentation property, never permissions, PCM routing,
device identity, or global state. Has/size/settable retain their existing contract.
The HAL binary contains no shipped bundle allowlist.

The service reads `MIXRELAY_TRANSPORT_COMPATIBILITY` at startup: a comma-separated
list replaces the default, and an explicitly empty value disables the workaround.
There is no whitespace trimming or wildcard matching. Maximum: 32 unique bundle
identifiers, each 1–127 ASCII bytes, with nonempty dot-separated segments of
letters/digits/hyphens. Malformed configuration fails startup before publication.
Set this environment variable for the service process (not merely the GUI), then
restart the service; rebuilding the HAL is unnecessary.

Inventory version 1 has an additive `transport_compatibility` XPC string array.
Missing and empty arrays both mean empty policy. Old v1 consumers ignore the new
key; new consumers accept old snapshots. Wrong types, invalid identifiers,
duplicates and oversized arrays reject the entire snapshot without replacing the
last valid inventory/policy. Existing authority/generation fencing also applies.
The local C snapshot ABI grows with the field; rebuild its callers together.

AddDeviceClient copies nullable callback bundle storage and records device/client,
PID and process start time; RemoveDeviceClient drops that record. Each lookup
checks PID plus `PROC_PIDTBSDINFO` start seconds/microseconds, so a recycled PID
cannot borrow a previous identity. Known callback identities across all devices
for that incarnation take precedence over discovery, matching the process-wide
scope of decisions.
Unknown identities use a bounded 64-request worker queue: `proc_pidpath` → enclosing
`.app` → `CFBundleIdentifier`. Discovery never holds IO registry locks. Sandbox
failures return `virt`; negative discovery retries after two seconds, with at most
one queued/running lookup per incarnation. A completed discovery answer stays
fixed until the allowlist changes or the last client for that PID is removed.
Queue saturation, resolver mutex contention, full decision storage, and a 20 ms
property-get deadline return transient `virt` without publishing a negative
decision; subsequent queries can receive the completed answer or retry admission.
The deadline starts at property-get entry, includes incarnation lookup and all
validation/admission work, and only its remaining time is used for completion.
Decision storage is bounded to 4096 entries. Last-client removal and recycled PIDs
reclaim entries; the single discovery worker also inspects at most 16 incarnations
per 10 ms cleanup step, outside the resolver mutex, including query-only requesters.
A full-table property read never sweeps processes or caches its refused answer.
IO/timestamp callbacks use balanced, nestable thread scopes and take the `virt`
fast path without process/filesystem work or resolver locks. `tran` validates
objects and addresses using fixed-slot pins and atomics before any snapshot,
including has/size/settable calls; it never takes the device control mutex.
Other property selectors retain the existing snapshot/control locking path.
BeginIOOperation
enters the cycle scope and EndIOOperation leaves it; DoIOOperation and timestamp
calls also scope their own work and clear it on every return. WillDoIOOperation
runs without marking its startup/control thread.
Changing the policy clears decisions and emits `tran` PropertiesChanged for live
devices after inventory/resolver locks are released.

### Installed-driver verification

1. Run `pnpm signing:check` before any release build. Build/install through the
   normal signed release workflow and restart/reload the HAL through the installer.
   This source task does not install or change the running driver.
2. `pnpm probe:macos-roundtrip --list` gives the installed device UID. Run
   `pnpm probe:macos-roundtrip --transport DEVICE_UID`: an ordinary terminal probe
   should print `transport='virt'`. This mode does not start audio or need microphone
   permission.
3. For a controlled bundle-resolution check, create a development `.app` containing
   `target/macos-probes/MRRoundTripProbe` as its `Contents/MacOS` executable and an
   Info.plist with `CFBundleIdentifier=com.example.MixRelayTransportProbe` and
   `CFBundleExecutable=MRRoundTripProbe`. Run that executable with
   `--transport DEVICE_UID`. Configure the service environment to that exact ID
   and restart it, then rerun: expect `usb ` if its sandbox allows bundle discovery.
   Run the terminal probe concurrently: it must still report `virt`. If discovery
   is denied, the bundle probe conservatively reports `virt`; this alone does not
   verify the callback-supplied identity path.
4. In each shipped-default app, open its audio device picker and select MixRelay.
   Confirm device visibility and successful capture/playback. Exercise with the
   app's real bundle identity supplied by Core Audio, including helper processes;
   a separate terminal probe cannot impersonate that requesting process.
5. Set the service override to empty, restart the service, and re-query/reopen the
   same apps: transport becomes `virt`. Restore the service's original environment
   and restart it. Check that PCM routing, format, UID and other device properties
   did not change. Inspect `tran` listeners for notification on each policy change.

A reproducible development bundle and service override (run these only on the Mac
being qualified, after recording any existing override):

```sh
# Build the read-only probe and choose the UID printed by --list.
zsh scripts/probe-macos-roundtrip.sh --list
mkdir -p target/macos-probes/TransportProbe.app/Contents/MacOS
cp target/macos-probes/MRRoundTripProbe target/macos-probes/TransportProbe.app/Contents/MacOS/
cat > target/macos-probes/TransportProbe.app/Contents/Info.plist <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.MixRelayTransportProbe</string>
<key>CFBundleExecutable</key><string>MRRoundTripProbe</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
```

For the installed LaunchDaemon, save
`/Library/LaunchDaemons/io.mixrelay.audio.driver.plist`, then add/set its
`EnvironmentVariables:MIXRELAY_TRANSPORT_COMPATIBILITY` string to
`com.example.MixRelayTransportProbe` with PlistBuddy. Reload that exact job:

```sh
sudo cp /Library/LaunchDaemons/io.mixrelay.audio.driver.plist target/macos-probes/driver-service.before-transport.plist
# For an absent override use Add; for an existing override use Set instead.
sudo /usr/libexec/PlistBuddy -c 'Add :EnvironmentVariables:MIXRELAY_TRANSPORT_COMPATIBILITY string com.example.MixRelayTransportProbe' /Library/LaunchDaemons/io.mixrelay.audio.driver.plist
sudo launchctl bootout system/io.mixrelay.audio.driver
sudo launchctl bootstrap system /Library/LaunchDaemons/io.mixrelay.audio.driver.plist
launchctl print system/io.mixrelay.audio.driver
# Substitute the actual UID in both commands; run them concurrently if desired.
target/macos-probes/TransportProbe.app/Contents/MacOS/MRRoundTripProbe --transport DEVICE_UID
target/macos-probes/MRRoundTripProbe --transport DEVICE_UID
```

The first should report `usb ` when bundle inspection is permitted, and the second
`virt`. To disable the policy, set that same string to empty (PlistBuddy
`Set :EnvironmentVariables:MIXRELAY_TRANSPORT_COMPATIBILITY ''`) and repeat the
bootout/bootstrap commands. Restore the original plist with
`sudo cp target/macos-probes/driver-service.before-transport.plist /Library/LaunchDaemons/io.mixrelay.audio.driver.plist`,
then repeat the bootout/bootstrap commands. The default-list test uses the five real apps
with the override absent. Re-query in each requesting app or attach a debugger to
its `AudioObjectGetPropertyData` result to inspect the four-byte `tran` value; the
standalone probe only reports its own requester result.
