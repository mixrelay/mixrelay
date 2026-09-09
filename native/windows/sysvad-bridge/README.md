# SysVAD transport integration (diagnostic, not product packaging)

This overlay connects the pinned Microsoft SysVAD framework to ToneNari's v2
shared PCM transport. It deliberately leaves the original reference checkout and
installed baseline untouched. Generate a **new** source directory with:

```sh
node scripts/lab/prepare-sysvad-bridge.mjs REFERENCE NEW_DESTINATION
```

The generator checks SHA-256 for each edited upstream file and rejects nonunique
anchors or an existing destination. It preserves Microsoft's MS-PL license and
copyright notices, plus the pinned WIL submodule. The result is an external lab
build input, not vendored/shipped product code.

An optional third argument supplies a version-1 native channel profile (for example
`infra/lab/windows-endpoint-profile-64-16.json`). This produces a separate build
artifact with coherent PCM widths and only the two bridged endpoint registrations;
it is **not** runtime user configuration or a shipping INF. See the
[64/16 ARM64 build and remaining native gates](../../../docs/validation/2026-09-07-native-endpoint-followup.md).

The separate `--rate-policy` opt-in adds private policy ABI v1 to the actual
SysVAD miniport. It supports 44.1/48 kHz negotiation and explicit native readback,
while retaining PCM ABI v2. Ordinary generated LAB sources do not enable it.
It does **not** configure native buffer/period size, change default devices, or
make a build into a qualified live capability. See the
[rate-policy validation and remaining load gate](../../../docs/validation/2026-09-08-windows-rate-policy.md).

## Boundary

- Speakers system-render and MicArray1 system-capture pins use the shared rings.
  Other sample pins/endpoints are **not** advertised as ToneNari-routed audio.
- Driver lifecycle, WaveRT buffers, timers, notification and position interfaces
  remain in SysVAD. Stop/removal marks the transport offline before upstream PnP
  cleanup. Stream reservation is released after queued DPCs drain.
- Pin instance limits report the transport's real capacity (one host stream per
  direction), instead of the original 6-render/4-capture promise. Windows shared
  mixing and ToneNari graph summation are separate from native pin instance counts.
- Stream creation preserves upstream-advertised sample-rate support. Controller
  attachment separately validates its rate against the negotiated native stream;
  the current v2 mapping accepts 44.1/48/96 kHz, not every sample discovery format.
- Render DMA integer PCM is converted to float **bits** for the shared ring;
  engine float bits are converted back for capture DMA. Valid bits are left-aligned
  in the PCM container. The conversion uses scalar integer arithmetic, not kernel
  floating-point calculations. NaN becomes silence for integer sinks; overflow
  saturates. PCM8 and unrecognized formats fail stream creation.
- Fractional frame bytes are retained between position updates (including 44.1 kHz),
  instead of splitting a sample between ring calls.
- Protected streams fail closed: the diagnostic shared mapping is not a protected
  audio path. Sample file recording is disabled for the bridged pins.
- The diagnostic control path is `\\.\ToneNariSysvadBridge`, restricted to SYSTEM
  and administrators. It does not replace `\\.\ToneNariAudio` or relax its ACL.
  Production engine/session authorization and endpoint configuration are unfinished.
- Owner-exit cleanup requires `/INTEGRITYCHECK` on the driver image. Keep this
  flag and the process-exit callback; do not work around it by disabling cleanup or
  weakening Windows code-integrity policy.

## Verification

The [native ARM64 bridge result](../../../docs/performance/2026-09-07-windows-sysvad-bridge.md)
records real PCM matches, remaining underruns, failed attempts and exact image hashes.

The shared conversion/copy code has portable ASan/UBSan tests:

```sh
mkdir -p target/native-tests
clang++ -std=c++17 -O1 -Wall -Wextra -Werror -fsanitize=address,undefined \
  -I native/windows/virtual-audio-driver/include \
  native/windows/virtual-audio-driver/tests/pcm_test.cpp -o target/native-tests/pcm-test
target/native-tests/pcm-test
```

`SysvadBridgeProbe.cpp` is a separate native controller. It sends stereo known
samples into the explicit lab render endpoint, reads the actual kernel ring,
maps source channel 1 with a factor of 0.5 into the capture ring, and compares
the samples arriving through the explicit lab WASAPI capture endpoint. It checks
three resource-lifecycle cycles and logs underruns/discontinuities. This is not
the Rust engine or an audio-continuity certification.

Open product gates include reducing and renaming endpoint tables/INF together,
removing unrelated sample capabilities, negotiated channel/rate reconfiguration,
productizing the opt-in Rust adapter, `/w` driver isolation, x64 validation,
release signing/HLK, failure injection and endurance. Retain failed job evidence.

The [Rust integration run](../../../docs/performance/2026-09-07-windows-rust-bridge.md)
now exercises the actual engine graph via `TONENARI_WINDOWS_SYSVAD_LAB=1`. The
native probe's `--external-engine` mode does not act as the shared-ring controller.

References:

- [Extensible PCM formats](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/extensible-wave-format-descriptors)
- [Process-exit callback integrity requirement](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/nf-ntddk-pssetcreateprocessnotifyroutineex)
