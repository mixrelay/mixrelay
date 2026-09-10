# macOS Aggregate planning boundary

Date: 2026-09-08. Continues the [resource refactor](2026-09-08-macos-resource-refactor.md).
No installed engine restart, user graph edit, default/rate change or VM startup.

## Implemented

`macos_devices/group_plan.rs` owns private-group compatibility before native
allocation: 2–32 distinct fixed UIDs and native device IDs, alive devices,
non-unknown/non-Aggregate transports, exact integral 8–384 kHz nominal rates,
and separate input/output totals bounded at 2048 without overflow. Member order
is retained; the output-first selection remains in `spec.rs`. Incompatible
members retain the independent-device path rather than changing hardware rates.

The C boundary now exposes a 32-byte member fact snapshot and accepts the
validated Rust plan. Creation rechecks native identity/rate/geometry against the
snapshot, then the existing readiness gate verifies the created composition,
drift policy, active members and channel totals before callbacks. A changed
device ID cannot silently satisfy an old plan just because its UID matches.
No native facts or policy queries were added to real-time callbacks.

Partial allocation still transfers ownership even on failure. The 128-cycle
fault test now passes invalid UTF-8 directly through the test-only ABI, failing
after allocation but before OS device creation. Previously it relied on absent
UIDs, which Rust now rejects before allocating; retaining that old test alone
would have stopped testing partial native cleanup.

## Validation

- macOS device unit boundary: **36 passed, 6 explicitly opt-in tests excluded**.
  Four new Rust tests cover ABI layout, ordering, invalid names/member limits,
  rate/transport/identity incompatibility, channel bounds and asymmetric I/O.
- Clippy all targets with warnings denied: passed, no gate/suppression changes.
- `check-macos-device-io.sh --live`: native ASan/UBSan checks and all four Rust
  live tests passed. Build: `/tmp/mixrelay-device-io-check.zhajQj`.
  Includes actual 32-sample callbacks after reconnect, four removal/reappearance
  cycles, 20 fresh startup fixtures, and 44.1/96/48 kHz graph phases with zero
  FIFO overflows against unchanged 48 kHz native I/O.
- Explicit private group using `BuiltInSpeakerDevice` and
  `io.mixrelay.audio.endpoint.default`: three deliberately stale local plans
  (identity, channel count, rate) rejected with partial owners retired; valid
  group delivered both callbacks on one native clock, retaining ownership until
  the final registration retired. Final group admission: **0**.
- macOS `leaks`: 512 native allocation/retirement cycles, 128 partial failures,
  and the stale-plan/real-group probe each reported **0 leaked bytes**. Physical
  footprints respectively 4864, 4624 and 8209 KiB. Restricted-process/readonly
  inspection warning applies; this is not long-duration/global leak proof.
- Final inventory: no probe/private-group objects left; default input/output
  IDs remained 145/150, and the inspected nominal rates were unchanged.

## Performance and remaining scope

This is a policy/ownership refactor, not a latency optimization. The same 48 kHz
stereo, 16×256-frame writes, 32 pump turns/no consumption probe remains **256
queued frames / 5.333 ms before and after** (baseline in the preceding report).
No CPU or physical round-trip improvement is claimed. Control preparation adds
readback to reject stale plans; its cost was not isolated from asynchronous OS
device publication. Unit tests establish bounded inputs and unchanged RT paths.

The C file is now 252 lines versus 245 before this slice: a native fact API and
stale-plan checks replaced the compatibility logic. Line reduction is not the
acceptance criterion. CF construction/composition readback/channel binding,
native callback/PCM and source-registry/control code remain C. Full Rust-first
migration, physical clock/long-duration qualification, installed-app acceptance
and Windows/Linux runtime qualification remain open.

## Requested follow-up: format coverage and structural refactoring

The user requested DAW-like buffer choices and higher sample rates after the
current boundary work, explicitly not a language-only rewrite. Current source
inspection finds macOS session/endpoint profiles in `main.rs` limited to
44.1/48 kHz and buffers 32/64/128/256/512; worker allocation and sample-count
validation also embed 512. `MREngineService.c` and the HAL driver independently
reject rates other than 44.1/48 kHz, although common conversion accepts up to
384 kHz. Therefore expanding the UI list alone would be false support.

Next acceptance targets, **not implemented by this slice**:

- Buffers 32/64/128/256/512/1024/2048, with native capability/readback and bounded
  preallocated worker storage; test every advertised value, not only 32.
- Rates 44.1/48/88.2/96/176.4/192 kHz through HAL property enumeration, accepted
  writes, clock/transport, graph, preferences and UI. The product maximum is
  192 kHz; 352.8/384 kHz are explicitly out of scope.
  Do not equate converter input range with native endpoint support.
- Separate format catalog/capability negotiation, resource transactions, native
  adapter and audio execution responsibilities; remove duplicated constants and
  the growing inline macOS module in `main.rs`. Preserve rollback, lifecycle and
  late-reply guarantees while restructuring, not just renaming C as Rust.

Reference behavior checked on 2026-09-08: [Ableton's buffer guidance](https://help.ableton.com/hc/en-us/articles/209070329-How-to-avoid-crackles-and-audio-dropouts)
lists powers of two from 32 to 2048 for Windows and notes ASIO driver-panel
constraints. [Logic's sample-rate contract](https://support.apple.com/guide/logicpro/set-the-project-sample-rate-lgcpce0958b8/mac)
sets the selected device's rate with the project and describes conversion when
the selected device cannot support a project's rate. These are behavior guides,
not evidence that every backend/device exposes identical lists.
