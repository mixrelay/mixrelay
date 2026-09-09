# Backend-owned virtual endpoint settings — 2026-09-07

This completes a control-plane slice, not cross-OS audio product qualification.

## Implementation

- `VirtualEndpointBackend::settings()` supplies graph-attachment creation policy,
  rename permission and supported channel counts. This is separate from transport
  capacity and from installing/deleting an OS device.
- The optional `audioSession.virtualEndpointSettings` field carries that policy
  through the existing status API. Older status payloads still deserialize; no new
  OS-name inference or separate UI-only capability table was added.
- macOS and Linux declare configurable 1–64 channel directions. The Windows LAB
  adapter declares fixed render=2/capture=1, no rename and no generic independent-side
  creation. Its existing strict native graph validator remains in place. This does
  **not** implement Windows variable-channel endpoint descriptors or installation.
- The controller retains the last accepted endpoint name, graph-node ID, device ID
  and directional stream configuration. Removal leaves the template intact;
  native staging rejection does not overwrite it. Partial attachments retain the
  absent direction's previous count. Reattachment uses the current session rate.
- UI recreation consumes that template instead of inventing stereo/default IDs.
  Options use backend policy, and the model also rejects unsupported settings.
  Polled policy changes do not increment graph revisions or serialize into audio
  configuration. Read-only channel fields retain their actual value rather than
  going blank when that value is outside the selectable choices.
- Tests preserve asymmetric 64→16 configuration, partial-side rename, and valid
  cables when reducing the channel count.

## Verification

| Boundary | Result |
| --- | --- |
| Local UI | 72 tests passed; TypeScript, Biome, UI-boundary check and production build passed |
| macOS host | audio-io 12 + control-protocol 2 + engine-service 19 tests passed; engine Clippy and native binary check, desktop cargo check passed |
| Windows ARM64 VM | `windows-rust-bridge-build-v16`: build, 19 library + 14 binary tests and Clippy passed; 2 interactive audio tests intentionally ignored |
| Ubuntu ARM64 VM | `endpoint-settings-v3`: 19 engine library tests, native binary test build (0 binary tests) and Clippy passed |
| Lab helpers | 11 focused guest-job/bridge script tests passed |

Ubuntu v1 failed because the older guest source cache lacked `render_demand.rs`.
After transferring that dependency, v2 passed. v3 also transferred the current
entrypoint and passed again. This was not worked around by disabling a module.
Native job exit markers were read as `0`, separately from UTM dispatch results.

Evidence: `/Volumes/Linear 1/test/tonenari/evidence/endpoint-settings-20260907`.
Failed Ubuntu v1 output is retained with the final Windows/Ubuntu logs.

## Explicit remaining limits

- Recreation settings are retained **within the engine controller's lifetime**,
  not durably persisted across engine restarts. An explicitly empty startup with
  no known endpoint template does not invent a device or advertise creation.
- Native Windows product driver configuration, generic LAB side removal/re-add,
  automatic PnP recovery and the previously measured startup underruns remain open.
- No live PCM rerun, installed-driver change, application restart or native visual
  acceptance was performed in this slice. Running applications need an engine
  restart to receive the new status contract; a UI build alone does not replace it.
- Debian/Kali/Arch, Windows x64, plugin editors and long-duration resource behavior
  were not requalified by these tests.
- Production UI still warns about its >500 kB JS chunk (756.19 kB minified in this
  run). This is not a performance improvement claim; the change adds control-plane
  settings and does not alter DSP or native PCM scheduling.
