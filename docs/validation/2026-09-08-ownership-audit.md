# Ownership and responsibility audit

This is a working acceptance ledger, not a completion report. The snapshot is
from the 2026-09-08 independent read-only review. Source moves and Rust ports
must preserve functionality and close the underlying ownership issues.

| Boundary | Evidence at audit time | Required repair / proof | State |
| --- | --- | --- | --- |
| Desktop capture prepare | `macos_audio.rs` removed an active meter/session before opening replacement; opening a new pump can also steal the existing source producer lease | Owned preparation, no active mutation or lease takeover before accepted commit; injected open/revision failure tests | Root-run transaction tests 6/6 passed; engine capture ownership migration remains separate |
| Capture reader contract | `ApplicationCaptureSession: Send + Sync` exposed `read_interleaved(&self)` although macOS native transport assumes one consumer | Type-enforced exclusive reader separated from shareable telemetry; no mutex added to the realtime PCM callback | Mutable public borrow + move-only macOS reader implemented; root-run reader tests 5/5 and exclusive-borrow compile contract passed; final review pending |
| macOS capture execution owner | Desktop GET graph prepares/commits capture, unlike engine-owned Windows/Linux capture | Engine-owned capture lifetime participating in graph activation; desktop permission/discovery only; HTTP/headless and desktop equivalence | Open |
| Session control / observation | `RuntimeStatusBackend` combines telemetry, setters, suspension, rollback and persistence responsibilities | Separate observation and owned format transactions; failure/rollback cannot resume incompatible audio | Open; correctness fixes partially tested |
| Resource preparation | Normal endpoint prepare uses manual cancel while format prepare has an owned token | Common explicit prepare/commit/cancel ownership model, cancellation on Drop, preserved active resources on failure | Open |
| HAL / service transport | Fixed singleton device globals; IPC and mappings mixed with device property/runtime policy | Rust per-device registry, owned mappings and callback handles; narrow native ABI only | Rust implementation in progress |
| Shared PCM epochs | Reset/watermark races, wrap without remote-reader quiescence, relaxed PCM publication ordering | Rust atomic ABI implementation, tagged publication, no generation reuse in the same mapping, replacement mapping on exhaustion, adversarial tests | Rust implementation in progress |
| Callback sink/source lifetime | Removed sink can leave old queued capture PCM; starved source can retain synchronized status | Cleanup after descriptor drain on every exit; nonfatal rearm race; short-read readiness reset | Repair and regressions in progress |
| Cross-platform publication | Pending/retired pointer publication repeated in Linux, Windows and shared engine | Small verified publication/reclamation primitive; platform-specific native policy remains in adapters | Open |
| DSP plan / execution | `render.rs` combines lowering, slot allocation, execution dispatch and dynamics | Separate plan construction, executor and DSP implementations while preserving RT allocation contract | Open |
| Audio contracts / presentation metadata | Audio IO package includes Windows icon/image encoding dependencies | Separate native inventory presentation from audio contracts and transport | Open |

## Acceptance rules

- Design and readability are entry criteria, not postponed cleanup. Before
  implementation, identify responsibility, ownership/lifetime, thread/process
  boundaries, dependencies and failure/cancellation/recovery behavior.
- Safe Rust is the default. Each unavoidable unsafe boundary needs a concrete
  validity, aliasing, lifetime and synchronization argument tied to its owner;
  generic "caller guarantees" comments are insufficient. Raw pointers and C ABI
  details must not become general upper-layer APIs. Review unsafe Send/Sync
  separately. C remains only where an actual ABI/toolchain constraint requires it.
- Review file, directory, module and crate responsibilities together. Domain
  contracts must not depend on OS adapters or UI; runtime dependencies must not
  absorb test/build tooling. File splitting or moving code is not proof of design
  improvement. Keep existing Clippy thresholds (15/5/80/200) and strict gates;
  do not replace architectural review with numerical lint success.
- After a failed build/test gate, freeze overlapping source edits and repair the
  failure before layering further work. One runner verifies the same frozen
  source and records actual exit status; earlier green checks do not cover later
  modifications.

- The product is unreleased. Previous internal implementations are not backward
  compatibility targets. Remove replaced code and update its callers, contracts,
  tests and build paths together. No old/new parallel implementation, compatibility
  alias, obsolete-code fallback, old-schema reader or backup/archive copy is allowed.
  Required product behavior and user data must survive; prior code need not.
- A test referencing replaced code is a test rewrite task, never a reason to keep
  that implementation. Apply this rule to every owned platform and layer, and
  explicitly reject compatibility-preservation reasoning during review.
- A Rust wrapper around the unchanged C implementation does not close a port.
- Retained C/Objective-C needs a concrete ABI/toolchain reason, not just that the
  system exposes C APIs. Registry, queue, clock and lifecycle policy belongs in Rust.
- Interprocess shared-memory ABI and in-process SPSC are different contracts;
  do not merge them just to reduce file count.
- Review fixtures, production paths and failure/retirement paths together.
- Count production versus test code separately when recording complexity.
  File length alone is not the acceptance criterion.
- No ignored test, lint exemption, raised complexity limit or unsupported live
  success claim substitutes for a repaired boundary.

## Evidence available

### Current integration checkpoint — 2026-09-08 16:46 KST

This checkpoint supersedes the older test counts below only for the named
boundaries; it is not live-audio or whole-product acceptance.

- Root ran the current `native_callback_and_hal_contracts` binary test: one test
  passed, zero ignored. It executed all five native fixtures: callback clock,
  device I/O, current HAL dispatch bridge, application clock, and audio ring.
  The bridge exercises 1,000 interruptions, invalid errors and stale callbacks.
  Earlier SIGTRAP failures were traced with LLDB to the fixture's suspended XPC
  object disposal. Cleanup now installs a handler and cancels before balancing
  suspension and releasing; no production exception or disabled assertion was
  introduced. Sanitizers instrument the native fixture code, not the Rust archive.
- Root ran endpoint-service tests after the explicit clock-transition contract:
  **8 passed, 0 failed, 0 ignored**. This includes real pending-MAX replacement,
  stalled-reader retirement bounds, 1,000 cancelled preparations, disabled
  directions, and stale mapping/clock PCM rejection. Independent review caught
  the pending-MAX getter defect; recovery now uses the actual replacement flag,
  not a statistics epoch that intentionally reports zero while pending.
- macOS test-support ownership/process tests: **15 passed, 0 ignored**.
  Native command-plan tests: **9 passed, 0 skipped**. Bundle strict Clippy passed.
- Current HAL lifecycle/property/clock suite: **32 passed, 0 ignored**. Common
  transport suite after the UNREADY transition barrier: **59 passed, 0 ignored**.
  The complete engine library suite at this checkpoint: **137 passed, 0 ignored**.
  Source-registry and worker-scheduling edits underway afterward need a fresh run.
- A further independent failure-path review found that process death while
  holding the interprocess producer claim leaves the claim busy. Same-mapping
  restart must request replacement instead of remaining UNREADY forever. Repair
  and a process-death regression are pending; successful normal-lifecycle tests
  do not close this failure path.
- Removed obsolete C implementations and tests, their shell runners, and stale
  current-build references. The generated feature matrix points to current Rust
  service tests and clock source. It still reports **31 open release gates**;
  this cleanup does not close unverified feature cells.
- Live read-only check still reports graph revision **97**, 96 kHz / 32 samples,
  microphone to virtual capture channel 1 and both headphone channels. No graph,
  session, default device or installed driver was changed. The running build
  still reports a fixed 512-frame transport latency; its removal is in progress.

Remaining before live acceptance: finish and recheck the source-owner Rust
cutover, complete real clock-transition and callback scheduling integration,
run the final current-binary gates and independent review, then paired install
and same-condition 32/256-sample PCM/counter measurement. No before/after audio
improvement is claimed at this checkpoint.

Read-only pre-install observation at 16:52:04 KST: 5,002.937 ms, unchanged
graph revision 97 / SHA-256
`650fd1504c7004dd878488de18bbf9e65bc4da2f3ff276e798c571b995a9a988`, 96 kHz /
32 samples. Underruns 3,122,124→3,122,402 (**+278**), overruns 28,605→28,627
(**+22**), discontinuities 24,897→24,919 (**+22**). Host load averages at the
end were 15.184 / 14.772 / 11.754. These are running-build counter observations,
not audible-defect counts, a controlled timing benchmark, or evidence of an
improvement over another observation of that same installed build.

Root-run control-server suite after adding endpoint-format contracts: 13 passed,
0 failed, 0 ignored. Unsupported format now maps to HTTP 422 rather than the
generic service-unavailable response; malformed input never reaches mutation.

Pre-port shared epoch ABI regressions: 3 passed; callback source/sink regressions:
10 passed. The independent review found additional races afterward, so these
results do not accept the final transport. They must be rerun against the Rust
cutover and extended adversarial cases.

Rust shared-buffer implementation: 9/9 tests passed, including eight-channel
concurrent overwrite, tagged stale watermark, pending rearm, and refusing all
generation reuse after exhaustion. Engine and desktop now link this Rust ABI
through the shared crate rather than compiling the C shared-buffer provider.
The installed app/HAL have not yet been replaced.

Engine endpoint registry: 10/10 tests passed with real anonymous mappings, owned
prepare/commit/cancel guards, independent devices and callback-pinned retirement.
This proves the registry primitive, not its unfinished live IPC/graph integration.
XPC inventory codec: 9/9 passed, including an isolated 100-iteration malformed
snapshot descriptor-cleanup test. Capture transaction tests: 6/6 passed.

The review covered the cited boundaries, not every native/plugin/OS installation
path. Final macOS live audio and cross-graph device tests remain required.

## Production bundle cutover

Root built the actual Universal 2 Rust HAL bundle at
`target/macos-hal-rust-cutover/ToneNariHAL.driver`: arm64 + x86_64, exported factory,
plist validation and strict ad-hoc signature verification passed. No installation
or CoreAudio restart occurred. The old shell builder was removed and its
package/install/check consumers now invoke the Cargo-owned builder.

The former production link compiled 5,935 lines across seven C implementation
files; the new link compiles only the 239-line ABI bridge (96.0% fewer C input
lines). This is a source-boundary measurement, **not** a runtime performance or
overall complexity claim. Rust modules still need independent responsibility
review; live latency/PCM before-after evidence is not available until paired
engine/HAL integration and installation finish.

Root native fixture/support suite: 15 passed, 0 failed, 0 ignored. Bundle command
tests: 4 passed, 0 failed, 0 ignored. The common transport suite passed 43 tests
before the final mapping-exhaustion additions; rerun is required after those edits.

Independent HAL/common review identified two inventory reconciliation defects:
partial native allocation failure can orphan a device, and replacing one of 64
devices can repeatedly fail before removals release capacity. Both are being
repaired with native-failure regressions; this review is not a completion signoff.

### Follow-up root verification

- Common transport after the exhaustion getter and fresh-mapping PCM regressions:
  **45 passed, 0 failed, 0 ignored**.
- HAL after inventory fixes: **21 passed, 0 failed, 0 ignored**. The independent
  reviewer confirmed both orphan/capacity defects are addressed and found no new
  actionable issue in the inspected HAL/common ownership paths.
- Rebuilt Cargo-owned ASan/UBSan native executables and ran clock, device and HAL
  contracts directly: all three passed, including 100,000 clock publications,
  384 callback/epoch/layout cases and 1,000 reconnect/rejected/coalesced cycles.
  This closes the earlier failing native epoch assertion: replacement compares
  mapping incarnation plus local tags, while the native lifecycle epoch remains
  strictly increasing. It does not claim that the whole engine binary test suite
  has passed while the service cutover is still being edited.
- Strict Clippy passed for HAL core, common transport and bundle command together.
- Built bundle factory/COM ownership probe passed on the **host architecture**;
  x86_64 has compile/link/export validation, not runtime execution evidence.
- Remaining live integration issue found during review: a mapping replacement
  can reuse local epoch values. Both directions and fallback processing must use
  one coherent mapping snapshot and identity-checked PCM operations so a control
  swap cannot feed a new mapping into an old converter state. Repair is in progress.
