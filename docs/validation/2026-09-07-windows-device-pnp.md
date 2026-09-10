# Windows native device invalidation acceptance — 2026-09-07

Status: **not qualified**. Ordinary WASAPI reconnect injection succeeds, but an
actual endpoint/parent PnP stop has not yet produced a valid native invalidation
test in the installed Windows ARM64 LAB VM. Unit tests are not substituted for it.

## Scope and implementation

- VM: `712A936E-9D91-4455-A8A9-A1F65EB60719`, interactive elevated `state303` session.
- Added two ignored native tests: input/output each require four real OS
  disable/enable cycles, new capture packets/render demand, retained native owner,
  silence/no clock during absence, and joined workers on removal while absent.
- Added a read-only six-role default-endpoint inventory test. Console, multimedia,
  and communications roles are recorded for both capture and rendering.
- Tests reject unrelated endpoint UIDs and use exact `MixRelay SysVAD Bridge LAB`
  identities. The parent fallback additionally pins hardware ID, service, provider,
  and installed driver path. RAII recovery and an independent PowerShell `finally`
  both restore the LAB endpoints. No driver installation, deletion, forced stop,
  default-selection API, host change, or security-policy change was used. One
  separately approved normal reboot occurred before v7, as recorded below.
- Correctness hardening: a failed `DeviceRender` cannot emit stale native demand
  even when its cumulative demand counter still has unread frames. A focused unit
  test covers healthy → failed → new native clock epoch.

## Actual attempts, including failures

| Run | Boundary | Result |
| --- | --- | --- |
| v1 | Compile and Clippy | Test compiled; three new helper style diagnostics corrected before native mutation. |
| v2 | Selected non-default `AudioEndpoint` devnode only | Windows accepted endpoint disable, but existing SysVAD WASAPI sessions did not invalidate within 15 seconds. Both acceptance tests failed; all eight endpoint states restored. This is not successful unplug proof. |
| v3/v4 | Exact LAB parent candidate | Safety guard found LAB endpoints are defaults for all six roles; stopped before PnP mutation pending additional user approval. |
| v5 | User-authorized LAB parent using `Disable-PnpDevice` | Both directions failed at the OS disable operation: HRESULT `0x80041001`. No reconnect cycle was credited. |
| v6 | Same exact parent using native `pnputil /disable-device` | Exit `50`; OS reports a previous operation awaiting system restart and an unsupported request. Enable also returned `50`; independent final inspection nevertheless confirmed enabled state, no disabled config flag, and all original endpoint IDs healthy. |
| v7 | After an explicitly authorized normal VM reboot cleared the pending bit | First LAB parent disable returned `3010` (restart required), not live stop. PnPUtil enable returned `50`; the output test refused the newly pending state. **0 passed, 2 failed; zero accepted recovery cycles.** A separate normal `Enable-PnpDevice` on the identity-checked parent succeeded and cleared the reserved disable flag. |

After v6, the helper explicitly checks `DN_NEED_RESTART` before another disable.
An already enabled exact root with `ProblemCode=0` and no disabled config flag is
recognized during cleanup, avoiding a misleading second enable error after an
operation which changed nothing. The actual PnP gate is not waived. The v7 run
establishes that this installed LAB parent requests a reboot instead of the live
stop needed by this acceptance test; it does not establish a general Windows or
physical hardware hot-plug limitation. Repeated reboots cannot count as the
required in-process recovery cycles.

## Authorized reboot and v7 restoration

Only VM `712A936E-9D91-4455-A8A9-A1F65EB60719` was normally restarted with
`shutdown /r /t 0` (no force). Before restart no MixRelay process or open app
window was observed. The boot timestamp changed and `DevNodeStatus` changed
from `0x0180210B` to `0x0180200B`, independently proving the restart bit cleared.
The existing user account then logged in normally; no authentication, auto-logon,
security, driver installation, host, or other VM settings were changed.

After the first v7 disable request, `Status=OK` and healthy endpoints concealed
`ConfigFlags=1`: the device had a next-boot disable request. This was **not**
accepted as restoration. The identity-guarded standard `Enable-PnpDevice`
succeeded once and independent inspection confirmed `ConfigFlags=0`. No direct
registry edit, second reboot, force operation, reinstall, or default-selection
API was used. The final state still contains `DN_NEED_RESTART`, but no reserved
disable flag. Do not perform an additional PnP probe merely to clear that bit.

The cleanup helper now checks both current status and the disabled config flag,
uses this verified normal enable fallback if PnPUtil rejects restoration, and
asserts the exact original endpoint IDs rather than only their count. These
post-v7 safety edits were parser-checked; no extra destructive probe was run.

## Final independent state

- Parent `ROOT\MEDIA\0002`: `Status=OK`, `ProblemCode=0`, `ConfigFlags=0`.
- `DevNodeStatus=0x0180210B`, including `DN_NEED_RESTART` (`0x100`).
- All **8/8** original LAB AudioEndpoint IDs present and `OK`.
- Six default-role endpoint UIDs are identical before reboot, after v7, and in
  a separate interactive inventory after the successful WMI restoration:
  - capture: `{0.0.1.00000000}.{f84b45c8-7f8f-40b8-81b3-c34c2619e10b}`;
  - render: `{0.0.0.00000000}.{e4d8a80d-caf2-4a38-aaf7-fc9a9a3ecf1c}`.
- No remaining `mixrelay*` process observed after the test run.
- Owned completed PnP scheduled tasks V1/V2/V3 removed after checking exact action,
  VM identity, and non-running state. Job directories/logs are retained.
- V7, restore V1, and post-restore inventory V1 owned scheduled tasks were also
  removed after exact action/non-running checks. Final checks exit 0: three
  PowerShell scripts parsed without errors, parent ConfigFlags 0, ProblemCode 0,
  eight original endpoints OK, and no remaining MixRelay process.

## Verification and limits

The isolated Windows source snapshot regression passed **76 tests** and Clippy
with warnings denied. Seven interactive tests are intentionally ignored in the
normal suite (including the added PnP/default inventory tests). The default-role
inventory was separately run in the real interactive VM and passed. The completed
separate v4 integration covers later graph-catalog changes and parameter durability:
**130 passed, 7 ignored**, build and Clippy passed; see the separate integration
record. These normal tests do not convert the failed PnP acceptance into a pass.

Existing injected native reconnect proof remains separate: four input reopen and
four output reopen cycles exercise actual WASAPI clients; they do not establish
that OS unplug notifications, parent stop/start, or real USB devices behave the
same. Physical 64-channel I/O, sustained multi-device clock drift, end-to-end
nonzero PCM through this PnP cycle, and x64 Windows remain unqualified.

There is also a bounded-shutdown limitation outside ordinary removal: after a
10-second initial native activation timeout, the current input/output startup
path detaches a self-owning worker whose synchronous COM/driver call cannot safely
be cancelled. Normal stop/recovery workers join, but a permanently hung native
initialization needs an isolation/broker design; this is not a no-leak guarantee.

Evidence is preserved under
`/Volumes/Linear 1/test/mixrelay/evidence/native-device-parity-20260907/windows-pnp/`:
failed v1–v6 logs, role snapshots, final root flags/endpoints, regression logs, and
owned-task cleanup. Failed attempts are retained rather than overwritten.
The reboot/v7 and successful restoration evidence is separately retained under
`/Volumes/Linear 1/test/mixrelay/evidence/native-device-parity-20260907/windows-reboot-pnp/`.

Native contract references: [WASAPI invalid-device recovery](https://learn.microsoft.com/en-us/windows/win32/coreaudio/recovering-from-an-invalid-device-error),
[PnPUtil command syntax](https://learn.microsoft.com/en-us/windows-hardware/drivers/devtest/pnputil-command-syntax).
