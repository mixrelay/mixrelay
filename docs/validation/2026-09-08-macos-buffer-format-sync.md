# macOS buffer property: integration counterexample and corrected boundary

## Result

**CoreAudio `BufferFrameSize` readback in the ToneNari engine process does not
prove that another application's native HAL callback quantum was changed.**
The attempted full sample-rate / buffer-pair registration was removed from the
macOS runtime after live integration contradicted that assumption.

ToneNari's own virtual endpoint uses direct shared memory, not an
`AudioDeviceIOProc` in the engine process. Its audio worker consumes real native
frame credits published by other applications' HAL I/O. Changing the engine
process's otherwise-unused CoreAudio client I/O buffer does not control those
applications. The app's buffer setting controls the **maximum ToneNari DSP
processing slice**; the worker preserves differently-sized native callback
remainders. Nominal sample rate remains a genuine device-global setting and
continues to use request, observed confirmation, and OS-to-app rate following.

Apple describes the I/O buffer as a choice made by each application for its own
I/O: [Technical Note TN2321, I/O Buffer Size](https://developer.apple.com/library/archive/technotes/tn2321/).
The public property description alone does not establish cross-process sharing:
[AudioHardwareDevice.bufferFrameSize](https://developer.apple.com/documentation/coreaudio/audiohardwaredevice/bufferframesize).

## Observed evidence, including the rejected inference

Before installation, a read-only process resolved exact UID
`io.tonenari.audio.endpoint.default` to device 651 and read 512 buffer frames,
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

- macOS `main.rs` registers only `with_format_control(set_device_rate)` and
  `with_observed_rate_following(read_device_rate)`. It does not query, set, or
  advertise the engine client's unused CoreAudio buffer as a native quantum.
- `blockFrames` remains the supported 64 / 128 / 256 / 512 DSP maximum. Native
  callback credits can differ and are sliced without discarding remainder frames.
- The shared full-format transaction API is retained for adapters which actually
  own the corresponding native I/O. Its fake-boundary tests are not proof of
  macOS cross-process buffer negotiation.
- `TNOwnDeviceBuffer.c` and its header remain **diagnostic fixture support**, not
  linked into the engine runtime. They target only the exact ToneNari UID.
- `TNOwnDeviceBufferTests` substitutes CoreAudio functions and covers exact UID,
  all four UI sizes, native range / settable / malformed read failures, rejected
  writes, and acknowledgement without changed readback. Native helper line and
  function coverage is 100%; ASan/UBSan passed. These tests never established
  cross-process semantics and are no longer described as doing so.

The full native script also builds the universal arm64 / x86_64 HAL. Existing
pacing regression processes 1,056 native frames in 64 / 96 / 257 / 511 / 128
quantums with a maximum 128-frame DSP slice and no lost remainder. This is a
correctness audit, not a CPU, latency or memory performance-improvement claim.

## Reproducible diagnostic probe

`TNOwnDeviceFormatProbe.c` is read-only with no arguments. Its JSON field is
explicitly named `clientBufferFrames`. An explicit `--set` changes the own
device's global nominal rate and **the probe process's client buffer only**.
It checks / confirms the observed values and attempts restoration on failure;
this does not make the buffer operation global.

```sh
probe_dir=$(mktemp -d /tmp/tonenari-own-format.XXXXXX)
clang -std=c17 -mmacosx-version-min=14.2 -Wall -Wextra -Wpedantic -Wconversion \
  -Wsign-conversion -Wshadow -Werror \
  -I native/macos/engine-service/include \
  native/macos/engine-service/src/TNOwnDeviceBuffer.c \
  native/macos/engine-service/tests/TNOwnDeviceFormatProbe.c \
  -framework CoreAudio -framework CoreFoundation -o "$probe_dir/probe"
"$probe_dir/probe"
```

For rate roundtrips, preserve graph, revision, accepted/preferred sample rate and
unrelated device UID/rate/default inventory. App-to-OS uses
`PUT /api/v1/audio/session` with the current revision and supported format;
OS-to-app changes only the exact ToneNari UID's nominal rate and checks the
follower. Restore the original app settings and verify graph/inventory. Buffer
checks verify the DSP maximum and variable native frame accounting separately;
**do not require fresh processes to report the engine's buffer value**. Physical
devices and default-device selections remain untouched.

Corrected engine rebuild / service-only restart and final rate roundtrips are
coordinated by the root task. This document does not claim those pending checks
have completed, nor that a global DAW buffer-size synchronization feature exists.
