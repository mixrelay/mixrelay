# Session format field following — 2026-09-07

## Boundary fixed

An open audio-session form previously initialized its fields only when the control
port changed. A later active-format observation (for example after an OS/DAW
change) updated the status summary but left untouched selectors at the old format.

The form now follows the observed active format per field. Edited fields remain
user-owned drafts; unedited fields follow. Status updates during an in-flight
apply are deferred until that request finishes. Replacing the control connection
resets both drafts, while stale replies remain fenced by the existing owner
generation. Observing a format never invokes a native reconfiguration.

This UI change does not establish native device negotiation, hardware buffer-size
control, sample-rate conversion, or cross-OS qualification. Those require their
own backend evidence. Buffer options still carry numeric DSP-frame values over
the existing protocol; their displayed labels include the localized frame unit.

## Verification

- Focused `AudioSessionSettings.test.tsx` and `AudioSessionControl.test.tsx`:
  **20 tests passed**.
- Added coverage for external changes without writes, dirty-rate preservation
  alongside untouched-buffer following, repeated polling, connection replacement,
  and an external change while an apply request is pending.
- Existing owner/unmount/rejection/activation tests remain passing.
- TypeScript, Biome, and Core UI boundary checks passed.
- Tests exercise the React component with a mock control port, not native OS/DAW
  interaction. No live sample rate, graph, default device, or service was changed
  for this verification.

## Native buffer observation follow-up

The wire type now accepts optional `observedDeviceBufferFrames` without requiring
older engines to supply it. A native buffer mismatch uses the existing
`block_size_mismatch` issue; it is not introduced as a second competing settings
form. Korean, English, and pseudo locale copy consistently says buffer size and
offers in-app reapplication. Sample-rate recovery copy no longer sends Windows
and Linux users to macOS Audio MIDI Setup. Stored-format failure copy no longer
claims a separate active/saved settings display exists.

Focused session-control, session-settings, and studio-engine-status tests:
**75 passed**. These cover the mismatch warning in all three locale resources,
the shared footer warning, and existing draft-following behavior. TypeScript
passed; the focused Biome formatter finding was corrected and rechecked.
This is UI/protocol verification, not proof of native buffer negotiation.
