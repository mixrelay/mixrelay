# macOS buffer property: integration counterexample and corrected boundary

## Result

This document preserves the 2026-09-08 integration finding, not a current startup
recipe. The singleton diagnostic implementation and its probe were removed on
2026-09-10: the current product uses independently identified devices and rejects
the old `default` identity. No running device or user setting was removed.

**CoreAudio `BufferFrameSize` readback in the MixRelay engine process does not
prove that another application's native HAL callback quantum was changed.**
The attempted full sample-rate / buffer-pair registration was removed from the
macOS runtime after live integration contradicted that assumption.

MixRelay's own virtual endpoint uses direct shared memory, not an
`AudioDeviceIOProc` in the engine process. Its audio worker consumes real native
frame credits published by other applications' HAL I/O. Changing the engine
process's otherwise-unused CoreAudio client I/O buffer does not control those
applications. The app's buffer setting controls the **maximum MixRelay DSP
processing slice**; the worker preserves differently-sized native callback
remainders. Nominal sample rate remains a genuine device-global setting and
continues to use request, observed confirmation, and OS-to-app rate following.

Apple describes the I/O buffer as a choice made by each application for its own
I/O: [Technical Note TN2321, I/O Buffer Size](https://developer.apple.com/library/archive/technotes/tn2321/).
The public property description alone does not establish cross-process sharing:
[AudioHardwareDevice.bufferFrameSize](https://developer.apple.com/documentation/coreaudio/audiohardwaredevice/bufferframesize).

## Observed evidence, including the rejected inference

Before installation, a read-only process resolved exact UID
`io.mixrelay.audio.endpoint.default` to device 651 and read 512 buffer frames,
settable, with an inclusive 15–4096 range. The app showed 256 frames. The initial
claim that these values proved a broken shared-device setting was **incorrect**:
they described different clients / processing boundaries.

The attempted implementation registered a full native-format getter/setter in
the engine and confirmed its own CoreAudio readback after setting 256 frames.
After coordinated installation, simultaneous observations of the same endpoint
showed:

| Observer | Nominal rate | CoreAudio client buffer |
| --- | --- | --- |
| Engine process / status reader | 48,000 Hz | 256 frames |
| Fresh read-only probe, own device ID 1177 | 48,000 Hz | 512 frames |

The actual getter called `AudioObjectGetPropertyData`; status did not fabricate
256 from the DSP setting. The defect was the **meaning assigned to that valid
client-local observation**. No standalone buffer setter was run by this audit;
the engine's temporary full-pair startup path made the 256-frame client request.

The initial post-restart CoreAudio device lookup also blocked during discovery
before recovering after roughly three minutes. Engine and fresh probe both
waited in CoreAudio. Source inspection confirmed the engine Mach listener was
already activated and HAL control attachment was asynchronous. This was not
established as a synchronous engine/HAL XPC deadlock. A polling deadline cannot
cancel a CoreAudio function blocked internally; no hard startup-time bound is
claimed.

## Corrected source and tests

- Current macOS startup registers `with_graph_format_control()` in
  `apps/engine-service/src/macos/mod.rs`. Device nominal rates are handled by the
  ID-directed virtual endpoint backend, not a singleton session setter.
- `blockFrames` is the DSP maximum selected from the current product format catalog. Native
  callback credits can differ and are sliced without discarding remainder frames.
- The shared full-format transaction API is retained for adapters which actually
  own the corresponding native I/O. Its fake-boundary tests are not proof of
  macOS cross-process buffer negotiation.
- Current client-buffer validation/readback and rollback policy lives in Rust
  `macos_devices/buffer_control.rs` and `buffer_transaction.rs`. Their tests cover
  invalid geometry, absent observations, bounded acknowledgement, rejected writes,
  cancellation and preservation of external changes. The native snapshot/setter
  boundary is `MRDeviceIO.c`; Rust fixtures do not prove every CoreAudio ABI failure.
- The removed singleton helper, header, standalone tests and manual probe were
  absent from current application and Cargo fixture build inputs. Their historical
  coverage figures describe only that removed implementation, not the current engine.

The historical native validation also built the universal arm64 / x86_64 HAL. Its
pacing regression processed 1,056 native frames in 64 / 96 / 257 / 511 / 128
quantums with a maximum 128-frame DSP slice and no lost remainder. This is a
correctness audit, not a CPU, latency or memory performance-improvement claim.

## Current validation boundary

For rate roundtrips, preserve graph, revision, accepted/preferred sample rate and
unrelated device UID/rate/default inventory. App-to-OS rate changes target the
selected device identity through its graph node; OS-to-app changes only that
device's nominal rate and checks all graph references. Restore the original app
settings and verify graph/inventory. Buffer
checks verify the DSP maximum and variable native frame accounting separately;
**do not require fresh processes to report the engine's buffer value**. Physical
devices and default-device selections remain untouched.

Corrected engine rebuild / service-only restart and final rate roundtrips are
coordinated by the root task. This document does not claim those pending checks
have completed, nor that a global DAW buffer-size synchronization feature exists.
