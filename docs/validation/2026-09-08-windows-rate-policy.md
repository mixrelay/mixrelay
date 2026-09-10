# Windows own-device rate policy: qualification in progress

This is **not** a completed Windows format/period capability. The currently
installed LAB adapter remains gated to its validated contract until a newly built
SysVAD image is explicitly approved, loaded and checked through native readback.
This change does not set a global Windows default device or arbitrary device mix
format. Buffer/period negotiation is separate and is not represented by this ABI.

## Actual framework and source boundary

The accepted [SysVAD reuse decision](../adr/0011-windows-sysvad-reuse.md) applies.
The initial intersection investigation started in the experimental `MrWave.cpp`;
that driver is not the product framework and its successful kernel compile is not
an endpoint-enumeration result. The new event-policy hooks have been removed from
that experimental miniport. Its independent exact-rate intersection bug fix is
retained. No experimental driver was installed during this work.

`prepare-sysvad-bridge.mjs --rate-policy` now generates a separate, hash-pinned
SysVAD overlay. The ordinary overlay does not enable the policy. In the opt-in
overlay, actual `CMiniportWaveRT` format intersection, `IsFormatSupported`, and
`PropertyHandlerProposedFormat2` select supported descriptor entries. Both actual
filter automation tables register SysVAD's existing formatchange event handler;
the callback uses the existing `IPortEvents` lifetime. Callback removal is
serialized against notification before releasing the miniport's port interface.

Capture's upstream raw table originally had only 48 kHz/32-bit PCM. The opt-in
overlay adds an actual 44.1 kHz/32-bit entry and widens its matching raw range,
preserving the last/default 48 kHz entry and existing mode indexes. Generated
channel profiles compute the corresponding byte rate/alignment, including 64ch.
44.1 and 48 kHz are the policy's supported pair; 96 kHz is not claimed.

Microsoft documents [KSEVENT_PINCAPS_FORMATCHANGE](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/ksevent-pincaps-formatchange)
as the driver-side format-change notification. Its effect on the actual installed
endpoints remains a native test gate, not an inference from the API name.
[IAudioEndpointFormatControl::ResetToDefault](https://learn.microsoft.com/en-us/windows/win32/api/audioendpoints/nf-audioendpoints-iaudioendpointformatcontrol-resettodefault)
resets manufacturer defaults; it is not an arbitrary sample-rate setter.
No undocumented `IPolicyConfig` or registry format mutation is used.

## Transaction contract

Private policy ABI v1 is separate from unchanged PCM mapping ABI v2. Its request
and status sizes are 32 and 48 bytes. PCM request/descriptor/header remain
24/40/112 bytes; compile-time assertions protect all sizes and state values.

- QUERY returns native render/capture pin rates separately from desired rate.
- BEGIN requires the queried generation, the same owning file/process, both
  miniport event providers and no existing PCM mapping. Only one transaction is
  permitted. Repeated same-owner/same-rate requests do not emit repeated events.
- During pending or rollback, mapping is blocked and native pin creation rejects
  the wrong rate. PCM v2 retains one shared duplex rate; mixed 44.1/48 native pins
  cannot be mapped as if they agreed.
- Commit requires both actual native pin rates to match. IOCTL acceptance alone
  is not success. Missing/old pins stay pending, not falsely applied.
- A monotonic deadline triggers one rollback notification. No idle timer polling
  occurs. Owner close/exit also requests rollback; process references and the
  policy worker are released/joined on teardown. If old pins never return,
  `RollbackPending` remains explicit instead of fabricating recovery.

`rate-policy-probe` is read-only by default. `--rate 44100|48000` is explicitly
mutating and must not be run before the target VM/load plan is approved. It checks
both actual rates plus stable policy state, and exits unsuccessfully on timeout.

## Evidence so far

- Host: 8 focused Node overlay/profile tests pass, including fail-closed changed
  anchors, raw 44.1 descriptor arithmetic, default preservation and callback shape.
- Host ASan/UBSan: portable policy transition/intersection tests pass; PCM 500,000
  conversion cases plus wrapped 1–64 channel copies pass.
- Windows ARM64 v3/v4/v5: EndpointsCommon + TabletAudioSample build, Universal driver
  validation, probe compilation, policy tests, ring 4,096 cases and PCM 500,000
  cases pass. v4 includes raw 44.1 support; v5 adds supported-rate consistency.
  This is compile/portable evidence only, not native renegotiation.
- Preserved failures: initial experimental build relay v1 passed kernel compile
  but failed a PCM-test narrowing warning (fixed with explicit byte cast).
  SysVAD v2 failed INF stamping because of archive AppleDouble files; subsequent
  archives exclude macOS metadata. Neither failure is hidden or called a pass.
- Package v5 signed SYS and passed Universal INF validation but Inf2Cat rejected
  the local-date/UTC boundary. The package script now uses the documented
  [/uselocaltime setting](https://learn.microsoft.com/en-us/windows-hardware/drivers/devtest/stampinf-command-options),
  and stops on negative as well as positive error codes. No clock/backdate change.
- v6 source archive: `sysvad-rate-policy-v6.tar.gz`, SHA-256
  `839be067dcf1efd7857f167746e981334a6eb7398d3d966f8a205951310fe290`.
  It also tests acknowledging a timed-out rollback by explicitly requesting the
  restored rate, without a redundant notification. Native v6 build/package and
  actual load/44.1↔48 renegotiation remain pending.

This is a correctness change, not a performance improvement claim. There is no
working before/after dynamic-rate device path for meaningful comparative latency
or CPU numbers. Focused transaction/ABI tests are the current alternative evidence.

## Load gate and rollback requirements

The generated INF retains `Root\mixrelay_SysvadBridge` and service
`mixrelay_sysvadbridge`, with control path `\\.\MixRelaySysvadBridge`. It is an
update to the existing LAB pair, not an additional coexisting policy device. Other
sample/experimental device identities must remain unchanged. Before any update,
resolve the exact present device instance and driver-store package, record the
currently loaded SYS hash and rollback package, then obtain root approval of the
specific signed artifact. Do not infer a usable kernel image from build success.

Read-only pre-update inventory (2026-09-08): present instance `ROOT\MEDIA\0002`,
service running, installed INF `oem24.inf` SHA-256
`0422CEF4189C080FD51AFACF46666BBA331674249EDB5A15FCEDA61E093DC098`.
Loaded SYS SHA-256
`E8BC1D1244659B26A50BB7FBE202061861CA3CB27B022AB31CC76F2B481C8F0E`
matches the previously recorded `sysvad-bridge-package-v5` rollback package;
its live package files still require hash confirmation before update.

Required subsequent native checks: initial policy/native rates; 48→44.1→48 with
actual MMDevice and both native pin readback; PCM conversion/continuity at both
rates; refused mismatched duplex mapping; old-pin/no-consumer timeout and rollback;
owner close/exit; unchanged other devices/default selection. x64, shipping signing,
driver isolation, period negotiation and endurance remain separate gates.
