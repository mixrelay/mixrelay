# Windows SysVAD transport bridge — 2026-09-07

## Result and scope

The diagnostic ARM64 SysVAD overlay now transfers **real PCM** through MixRelay's
shared transport in the authorized Windows VM. This is not the Rust engine adapter,
product endpoint configuration, cross-OS certification, or an audio-quality pass.

Native path tested: explicit LAB WASAPI render (48 kHz, stereo float) → SysVAD
16-bit DMA → float shared ring → source channel 1 × 0.5 → mono shared ring →
SysVAD 16-bit DMA → explicit LAB WASAPI capture (48 kHz, mono float).
Input constants are +0.25 / −0.125; the expected capture is +0.125. The sample's
capture tone and file-recording sink are bypassed only for these selected pins.

`sysvad-bridge-audio-v10` returned 0 with the following three 3-second cycles:

| Cycle | Render matches | Capture matches | Captured frames | Render overruns | Capture underruns | Discontinuities |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 141,585 | 140,530 | 142,560 | 0 | 5 | 1 |
| 1 | 142,017 | 141,010 | 143,040 | 0 | 3 | 1 |
| 2 | 141,585 | 140,530 | 142,560 | 0 | 3 | 1 |

Shared-mode startup produced bounded intermediate amplitudes with the correct
stereo ratio (first observed +0.001037598 / −0.000518799 at frame 560). The probe
allows a settling interval of at most 24,000 frames, checking bounds, polarity and
render channel ratio. Settling frames do not count as matches. At least 48,000
matched frames in **each** direction are required per cycle; nonzero unexpected
amplitudes after settling fail. v10 observed 1,013 render settling frames per cycle
and 1,982 / 2,030 / 2,030 capture settling frames. This does not prove transparent
startup or continuous audio. No system mixer or physical endpoint settings were changed.

## Implementation and failures retained

Follow-up `sysvad-bridge-audio-v11` also returned 0: 20 controller processes mapped
and exited through `ExitProcess` without C++ cleanup, and every subsequent mapping
succeeded. Three further PCM cycles then matched 141,490 / 141,490 / 141,010 capture
frames. Render overruns remained zero; capture underruns were 3 / 4 / 3 with one
discontinuity each. This verifies recovery after process death at smoke depth, not
zero kernel-pool growth, long-run leak freedom, or forced driver removal. Probe
build v9 compiled successfully with `/W4 /WX`.

- The overlay generator validates pinned upstream file hashes and unique anchors,
  writes a new external source copy, and preserves upstream MS-PL/WIL licenses.
  The clean reference checkout and original installed SysVAD remain unchanged.
- SysVAD owns PortCls, WaveRT buffers, timing and position interfaces. MixRelay owns
  the private shared-ring layout, controller ownership and PCM conversion boundary.
- Integer PCM conversion handles 16/20/24/32 valid bits, including left-aligned
  containers, without floating-point arithmetic in the native conversion routine.
  NaN → silence, finite overflow/infinity → saturation, integer quantization → zero.
- Initial build failures were real include-order defects: PortCls must precede KS
  headers, and the adapter's `PUT_GUIDS_HERE` must precede added bridge includes.
- The first signed build installed but failed PnP with code 37 / access denied.
  Its PE DllCharacteristics were `0x4160`, missing FORCE_INTEGRITY. Adding
  `/INTEGRITYCHECK` produced `0x41e0`; the process-exit callback registered and the
  device subsequently reported PnP 0, service Running. Cleanup was not disabled.
- `GetMixFormat` then failed with `0x88890008`. The bridge wrongly rejected some
  upstream-advertised discovery rates, while pin descriptors promised six/four
  streams despite a one-stream-per-direction transport. Accepting validated native
  formats independently from controller-map rates and matching pin instance limits
  resolved negotiation. Both changes were installed together; this test does not
  attribute the result exclusively to either change.
- Driver updates v2/v3 requested reboots. Graceful VM reboots were performed; no
  additional signing-policy, Defender, HVCI, TPM or host configuration changes were made.
  Hot removal/unload with the separate control device still needs qualification.

## Reproduction and evidence

Source revision: Windows-driver-samples
`197ba2156a60e2b76fcd4820bae594223e91a1e9`, WIL
`3c00e7f1d8cf9930bbb8e5be3ef0df65c84e8928`.
Overlay source v12 archive SHA-256:
`278c0b193cbf64acfe9ea68fe8db0daa9d7c57a61f152202765ce60484066806`.
Build `sysvad-bridge-build-v8`, package `sysvad-bridge-package-v5`, update v3.
The installed and packaged SYS hashes match:
`E8BC1D1244659B26A50BB7FBE202061861CA3CB27B022AB31CC76F2B481C8F0E`.
ARM64 MSVC `/W4 /WX` native probe build v8 passed. Universal INF `/u`, catalog
membership and signatures passed; `/w` isolation remains an unmet product gate.

The separately installed `Root\mixrelay_SysvadBridge` is `ROOT\MEDIA\0002`.
The old experimental MixRelay device (`0000`) and unmodified sample (`0001`) remain.
The eight upstream sample endpoints are not eight product-routed endpoints; only
Speakers/system-render and front MicArray/system-capture are bridged. Probe selection
checks explicit endpoint IDs and parent identity, never the default device.

Logs, completion markers, failed jobs and installed-image snapshot are retained at
`/Volumes/Linear 1/test/mixrelay/evidence/sysvad-bridge-20260907` and under
`C:\MixRelayLab` in VM `712A936E-9D91-4455-A8A9-A1F65EB60719`.
Guest logs are CP949; decode when inspecting localized names.
The owned demand-only `MixRelayLabSysvadBridgeAudio` scheduled task was removed
after v11 completed. Cleanup resolved the owner's SID, checked the exact action
and non-running state, and confirmed absence (cleanup v1, exit 0). Drivers and
evidence remain; the checked-in launcher can recreate the diagnostic task.

Local verification:

- 500,000 integer/float conversion cases plus wrapped 1/2/8/16/32/64-channel copies:
  Clang ASan/UBSan pass.
- 4,096 existing ring wrap/underflow/corrupt-counter cases: Clang ASan/UBSan pass.
- Lab regression suite: 34 tests, including lifecycle/cleanup safety guards.

These are correctness measurements, not a performance improvement claim. There was
no functioning MixRelay end-to-end driver baseline for a comparable latency/CPU
benchmark. The unchanged SysVAD capture-tone smoke is not equivalent to this routed
PCM workload. No zero-leak, zero-dropout, low-latency or physical-clock claim is made.

## Remaining product gates

Rust `VirtualEndpointBackend` v2 mapping integration; authenticated session ownership;
one real configurable endpoint pair with matching INF/tables; name/channel/rate
reconfiguration; native 1–64-channel and multi-rate matrices; clock/underrun behavior;
controller failure and hot-unload endurance; x64; `/w`, release signing and HLK.
The Windows engine capability remains disabled until its own integrated path passes.

References: [format contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/extensible-wave-format-descriptors),
[callback image-integrity requirement](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/nf-ntddk-pssetcreateprocessnotifyroutineex).
