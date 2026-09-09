# ADR 0012: Clock ownership belongs to the native execution boundary

- Status: Accepted direction; migration incomplete
- Date: 2026-09-08

## Contract

The UI's input/output nodes and cables do not prescribe how native devices are
opened. A hardware output remains a first-class output node even when its native
I/O shares an Aggregate with other nodes. An in-app sample-rate setting is a
convenience for negotiating the selected device/session, not a second meaning of
hardware sample rate. Requested settings must be confirmed against native state.

Inspect the format actually delivered at a boundary, not the captured program's
project/file/oversampling rate. Distinguish nominal rate, stream timeline and
clock-conversion ownership. Equal nominal rates or clock-domain zero do not
prove synchronization. A nonzero hardware clock-domain match describes the
ability to synchronize; do not infer a configured hardware lock from it alone.

1. Prefer a compatible common nominal rate subject to device ownership and the
   user's selected configuration. Do not repeatedly overwrite changes by a DAW.
2. Reuse one device timeline or native synchronization where available.
3. Apply nominal SRC only at a boundary that still has different delivered rates.
4. Apply adaptive correction only across an independently paced boundary. An OS
   converter and a ToneNari converter must not both control that same boundary.

## Platform direction (not an implementation completion claim)

- macOS: Core Audio device I/O and Process Taps. Prefer a private, explicitly
  owned Aggregate for compatible multi-device execution, with a verified primary
  clock and subdevice drift configuration. An Aggregate used just for app capture
  does not synchronize a separately opened hardware output. User Aggregates must
  not be modified silently. Unmanaged/heterogeneous boundaries retain a bridge.
- Windows: event-driven WASAPI Shared for concurrent app routing; preserve the
  distinction between client format and endpoint mix/device format. ASIO or
  Exclusive hardware access requires its own ownership/format constraints. Native
  rate-adjustment APIs are mechanisms, not automatic multi-endpoint drift policy.
- Linux: participate in the PipeWire graph's scheduling and negotiated format;
  reuse its rate matching on server-owned boundaries. Do not add a second local
  oscillator/controller merely to imitate another platform. This is a backend
  requirement, not a different implementation for each distribution name.

## Landed first migration slice

Both native macOS capture and hardware IOProcs retain buffer sample/host time.
They use validity flags, detect gaps/backwards time, and publish bounded atomic
snapshots/counters. No wall-clock arrival-time estimator, allocation, property
query or mutex was added to callbacks. Timestamp discontinuities cause the
existing worker-owned bridge/transport epoch to restart even without a format
change or FIFO overflow. Missing timestamps are not fabricated or interpreted as
proof of a shared clock; the existing frame-demand path remains operational.

Hardware device sets can now recognize another IOProc of the same active,
non-aggregate native device as sharing the selected pacing device's timeline.
They avoid its otherwise redundant adaptive sinc bridge. Proof is cleared on
connection loss and is not inferred from matching rates, unknown transport,
unknown clock domains, or an unverified Aggregate object ID. The graph uses only
atomic metadata; native queries remain with the connection owner.

## Still open

The first-slice status below is historical. The subsequent
[resource ownership refactor](../validation/2026-09-08-macos-resource-refactor.md)
adds fixed-UID private grouping, client buffer transactions and separate recovery
handoff. Its limitations and Rust-first migration work remain explicit there;
this does not close the full platform migration.

Automatic private-Aggregate grouping/channel offsets/lifetime, physical-device
rate negotiation/rollback, verified OS-managed conversion ownership, and the
equivalent Windows/PipeWire migration are **not implemented by this slice**.
Timestamps currently support continuity and diagnostics, not cross-device
sample/host slope estimation. Existing queue feedback remains the controller for
independent paths. A valid timestamp is not a global synchronization guarantee.
No platform capability flag or UI status was changed to hide these gaps.

See [focused validation](../validation/2026-09-08-macos-clock-boundaries.md).

## References

- [Core Audio Aggregate configuration](https://support.apple.com/en-gb/guide/audio-midi-setup/ams094c7edb4/mac)
- [Core Audio Process Taps](https://developer.apple.com/documentation/CoreAudio/capturing-system-audio-with-core-audio-taps)
- [WASAPI architecture](https://learn.microsoft.com/en-us/windows/win32/coreaudio/user-mode-audio-components)
- [WASAPI clock adjustment](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudioclockadjustment)
- [PipeWire rate and scheduling configuration](https://docs.pipewire.org/page_man_pipewire_1.html)
