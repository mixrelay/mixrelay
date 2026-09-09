# macOS virtual input activity and starvation accounting

## Defect and changed contract

The macOS timer worker returned zero transferred render frames whenever the HAL
publication watermark had not reached the requested block. The renderer counted
every such result as input starvation, including when no OS client had started
rendering. A missing block alone cannot establish that a producer was expected.

`TransferOutcome` now carries `InputActivity::{Unknown, Inactive, Active}`.
`complete(frames)` remains Active for existing adapters. The renderer still
validates transfer sizes and zeroes missing input, but increments input starvation
only for a short transfer with explicit Active demand. Neither zero sample
amplitude nor an unchanged watermark is used to infer activity. Output overflow
and discontinuity accounting remain independent.

The macOS HAL publishes activity through the former 32-bit reserved word in the
version-2 shared audio buffer. Compile-time offset/size assertions preserve its
layout (activity at byte 28, epoch at byte 32, header size 72). Publication uses a
release store and the service getter uses an acquire load; the realtime operations
remain bounded and lock-free. An older HAL leaves zero, meaning Unknown.

- HAL initialization and last StopIO: Inactive.
- First StartIO: AwaitingRender (no render demand yet), mapped to Inactive for
  missing-input accounting. Capture-only clients do not establish render demand.
- A nonempty actual WriteMix request: Active, including all-zero audio.
- A client stops while others remain: Unknown; remaining clients may only capture.
  The next actual render request establishes Active again.
- Unbinding a transport marks its producer evidence Unknown. Rebinding publishes
  the retained HAL lifecycle state into the new mapping.

The engine service returns the native evidence with the actual transferred frame
count. Unknown or inactive missing input remains **zero transferred frames**;
it is not fabricated as a successful full block.

## Focused verification

`cargo test -p tonenari-audio-engine -p tonenari-audio-io`: 39 tests passed
(22 engine, 17 audio-io), including the new 375-cycle regression.

The regression uses the same 48 kHz stereo, four-frame DSP fixture, same 375
consecutive cycles, same zero-valued source and complete output for both counting
rules. It executes the production renderer and compares its count with the exact
old short-transfer rule on the same inputs:

| Native evidence / transferred frames | Previous rule | New renderer |
| --- | ---: | ---: |
| Unknown / 0 | 375 | 0 |
| Inactive / 0 | 375 | 0 |
| Active / 0 | 375 | 375 |
| Active / 4 (all zero) | 0 | 0 |

This is a deterministic counter-correctness comparison, not a throughput benchmark
or a live 128-frame/one-second device measurement. Every cycle also checks zeroed
PCM output and unchanged zero output-overflow count.

`cargo check -p tonenari-engine-service --all-targets` and strict clippy for
audio-engine/engine-service passed on the macOS host.

Native tests cover initial/unknown/invalid states, every activity value across an
attached view, activity publication across forked shared memory, zero-frame versus
all-zero nonempty writes, lifecycle transitions, unbind/rebind, and the driver's
actual StartIO/StopIO functions. `scripts/check-macos-native.sh` runs ASan/UBSan,
100% isolated C source line/function coverage gates and universal HAL compilation.

## Limits

No installed HAL was replaced, no native service or coreaudiod was restarted, and
no user's audio graph was changed for this check. Live updated-HAL idle/active
counter measurement is therefore still pending deployment coordination.

Unknown activity is not proof of idleness or health. Suppressing an unsupported
starvation inference does not synchronize the engine timer to the native device,
eliminate clock drift, or certify zero dropouts; existing clock warnings remain
applicable. Windows/Linux activity implementations have separate native evidence
and validation owners.
