# Runtime, persistence, and application lifetime integration

This record covers a completed implementation slice, not full cross-platform product
qualification. Installed vendor editors, physical multi-clock hardware, Windows product
driver packaging, and unqualified Linux distributions remain separate acceptance work.

## Native state ownership

- Native close observations carry worker instance and close generation. A requested close
  is not a confirmed close, and replacement workers cannot consume an older observation.
- The engine owns an off-real-time close observer independently of desktop polling. It
  retries failed saves fairly, prunes removed identities, and joins on orderly shutdown
  before the audio worker is destroyed. The final checkpoint retains its controller owner.
- Explicit close checkpoints before closing. Failed storage/export is reported, not silently
  acknowledged. Ordinary graph edits preserve live presets; only explicit parameter deltas
  override captured state. Undo/Redo carries those deltas through pending graph applications.
- Snapshot IPC has a deadline; filesystem contention and control mutations can delay join.
  This is not a guarantee against forced termination, power loss, or a service manager's
  shorter termination deadline. Native window gestures still need real vendor UI testing.
- HTTP native state, graph, and catalog transactions run on blocking control tasks. A
  single-thread Tokio regression confirms that a blocked editor checkpoint does not prevent
  status responses. Controller mutation remains serialized rather than bypassing durability.

Source/test details: `2026-09-07-native-state-checkpoint.md`,
`apps/engine-service/src/editor_checkpoint.rs`,
`crates/processor-host/src/editor_observation/tests.rs`, and
`crates/control-server/src/lib.rs`.

### Actual Apple AU checkpoint and restoration

Root independently ran `cargo run -p tonenari-engine-service --example native_au_checkpoint
-- --run` successfully. The opt-in example only permits Apple's in-box AUPeakLimiter and
uses a newly owned temporary catalog, isolated native workers, and no audio devices, HTTP
server, or editor windows. The fixture changes real parameter 0 and compares native
ClassInfo exports (236 base64 bytes):

1. Set 0.37 → explicit close checkpoint → switch graphs → new worker restores identical state.
2. Set 0.91 → exported native state differs, demonstrating actual native mutation.
3. Explicit Undo to 0.37 during graph replacement → native state matches the original.
4. Finish the owned editor monitor → disk checkpoint → fresh controller and worker restore
   the same native state.

No fixture processes remained after completion. The root-run final catalog is preserved as
`macos-final-qWpjVN/native-au-catalog.json` in the lab evidence root. This confirms actual
native export/restore through the controller, worker IPC, and disk, but not GUI close-button
observation: Apple's generic UI is deliberately not exposed as a vendor editor here.

## Application identity and availability

- Windows uses exact executable paths with owned process handles and creation timestamps.
  Independent same-executable roots are ambiguous, not silently selected. Linux prefers exact
  application IDs, otherwise a verified executable path; stream registry changes replace only
  the affected owned capture resources. Neither OS requires a graph reapply after a matching
  application's ordinary exit and relaunch.
- Saved PID-only selectors are editable but require explicit reselection. All three native
  adapters check this before PID lookup or opening capture. Original nodes, edges, gain,
  document revisions, and disk contents are preserved on read. See
  `2026-09-07-restored-application-pid-safety.md`.
- Application waiting/ambiguity and failed DSP ownership have explicit session issues.
  Known stopped/failed processing takes precedence over a pending graph in the UI. Silence
  is never used as proof of either a healthy or failed device.

### Windows ARM64 v12 and final v15

Evidence root:
`/Volumes/Linear 1/test/tonenari/evidence/native-device-parity-20260907/windows-app-metadata-jF1k98/`.
Root inspected `v12-output.log`, `v12-exit-code` (0), and independently rehashed the manifest.

- Seven common/native packages build and strict Clippy pass; 193 tests pass, 8 opt-in tests
  are ignored in the ordinary run.
- A separately enabled real WASAPI test confirms owned silent-process capture transitions:
  unavailable → active epoch 1 → ambiguous → active epoch 2 → unavailable → active epoch 3.
  This proves native lifecycle, not nonzero PCM transmission.
- 100 native metadata queries retain handles/GDI/USER at `(208, 40, 0)` before and after.
  This is a bounded resource check, not an unlimited leak-free claim.
- Archive SHA-256: `c642f54430c97d90d7b8fcc390ee0950cc1818ff55b34f2068f77f42a93ec1a5`.
  All Windows and shared implementation files in the 103-entry snapshot matched at root
  review. Two Linux-only files and the Linux-only LV2 test fixture changed subsequently;
  the full snapshot is therefore not claimed identical to the final working tree.

The subsequent v15 qualifies nonzero process-loopback PCM, not only lifecycle flags.
Root inspected the final log/exit code 0 and independently matched all 105 manifest entries
to the working tree at review time. Seven-package build and strict Clippy pass; 193 ordinary
tests pass with 10 opt-in ignored. The metadata check, silent lifecycle, and nonzero parent
fixture are then each explicitly enabled and pass (the parent launches two owned children).

| Same capture owner | Frames | L/R peak | L/R RMS | Measured L/R frequency |
|---|---:|---|---|---|
| First owned process, epoch 1 | 24,000 | 0.004 / 0.002 | 0.002796 / 0.001398 | 978 / 490 Hz |
| Relaunched process, epoch 2 | 24,480 | 0.004 / 0.002 | 0.002825 / 0.001412 | 1496.08 / 747.06 Hz |

The generated frequencies are 1000/500 Hz, then 1500/750 Hz; the fixture checks channel
ratio and frequency within bounded tolerances. It proves actual current-process signal
recovery and absence of the old ring after exit, not bit-exact PCM, dropout-free streaming,
or clock accuracy. Capture reads return zero frames and zero-filled data between processes.
The exact LAB virtual speaker UID is explicitly supplied and validated; there is no default
or physical endpoint fallback. PnP, defaults, security, and services are not changed.
The cleanup record confirms zero owned player processes and fixture directories afterward.

- Evidence: `v15-output.log`, `v15-exit-code`, `v15-manifest.json`,
  `v15-signal-cleanup.json` under the Windows evidence directory above.
- Archive SHA-256: `c02b0a299eb5ddc481fe4811776e3eae3573315ed6d3fa5ab5fb9caa7d9e91cf`.
- Manifest SHA-256: `7815d5a6a8a8af524904abaeefced9d475ed17280b0441aaa2b2cae3dbda099c`.

### Ubuntu ARM64 v7

Root inspected `linux-integration/application-state-v7.log`, exit code 0, both registry
snapshots, and independently verified archive/log SHA-256 under the same lab evidence root.

- Seven packages build and strict Clippy pass: 192 tests pass, 2 opt-in ignored.
- A private PipeWire/D-Bus/WirePlumber session exercises actual production capture-watch,
  prepare/replace/read functions: 128 frames at peak 0.125 with one tap → exit gives zero
  frames/taps → a new PID recovers 128 frames/0.125/one tap → exit returns to zero. The graph
  generation is unchanged; stream activity is Active while flowing and Unknown when absent.
- The fixture explicitly drives capture-watch polling. It does not qualify the full HTTP
  service, desktop GUI, production timer interval, or a measured relaunch latency.
- After cleanup, the registry contains only the original lab dummy audio node and no devices
  or leftover capture nodes. Raw snapshots are not byte-identical: the dummy goes suspended →
  idle and each `pw-dump` has a different client identity. All owned session processes are
  reaped; physical device discovery was disabled in the isolated lab configuration.
- LV2: two pure state tests pass, plus the explicitly enabled real Lilv-loaded C plug-in
  fixture passes opaque five-byte state and input-gain restore/native failure propagation.
  An initial fixture incorrectly inferred an `i32` array before a void-pointer cast; fixing
  the fixture to `u8` resolved that failure without changing production logic. Vendor Clippy
  passes with only the pre-existing long-function lint exception documented by its maintainer.
- LV2 file assets, nested URID containers, nonportable values, and incompatible endianness
  remain explicitly rejected. This result does not qualify those state types or vendor UIs.
- Archive SHA-256: `acc04b3c5d8074de766f77adc360bfe8a8252564294185a88dda17620df6a219`.
  Log SHA-256: `767b3523a16a5858c2bac6dc66cccd0dcacb8e051df6dacf47d64b50c7157e7f`.

## macOS development engine deployment

The existing launch daemon used the workspace development executable. No HAL installation,
coreaudiod restart, default audio change, or service configuration change was performed.

- Built `tonenari-engine-service`, backed up its prior binary and exact graph/catalog, then
  sent TERM only to the verified owned service PID 40420. Its existing launch daemon restarted
  it as PID 76241 with last exit code 0.
- Deployed binary SHA-256:
  `d1be43d8e933f0f0d09e1ccf841cd2798e22a2ecb4cd5430afc4bfd29b953c98`.
- Graph response and catalog file remained byte-for-byte identical: revision 2, the existing
  duplex 2-channel endpoint, and both explicit channel cables. Backups and before/after
  status are under `native-device-parity-20260907/macos-final-qWpjVN/` in the same lab root.
- HTTP status returns driver ready, processing running, zero underruns/overruns. The retained
  older HAL supplies Unknown input activity; this demonstrates no false starvation counting,
  not updated HAL activity qualification or synchronized clocks. `clock_not_synchronized`
  remains reported honestly. No sample-rate-conversion/drift-correction claim is made.
- The new restored-PID macOS guard test passes without opening a native process tap; desktop
  binary strict Clippy passes. Existing desktop command-proxy tests also passed separately.

The deterministic equal-condition activity comparison is recorded in
`2026-09-07-macos-input-activity.md`: 375 idle/unknown short reads produce 0 starvation
events after the change (375 before), while 375 explicitly active short reads still produce
375 events. An old accumulated counter and a newly restarted zero counter are not themselves
a valid before/after performance comparison.

## Actual browser boundary and UI regression

- The browser could receive HTTP 200 yet fail every control request because native fetch was
  invoked with the control-port object as its receiver. Calling the captured fetch as a plain
  function fixes the receiver. The regression failed before the fix (1 failed/4 passed) and
  passes after it (5 passed).
- Root reloaded the real `http://127.0.0.1:1420/` UI against the newly restarted engine. The
  saved graph and cables load, Engine Ready is shown, and Add node → Processor opens the
  actual Apple Audio Unit catalog. No graph mutation was performed during this UI check.
- Measured menu DOM: 518 px client height, 1224 px content height, `overflow-y: auto`,
  bounds y=156–676 in the 720 px viewport. Screenshot inspection shows the long list and
  scrollbar inside the menu, not an expanded document. This is browser evidence, not a
  substitute for native Tauri, Windows/Linux pointer, DPI, or vendor editor qualification.
- Root's initial integrated UI run: 38 files / 271 tests pass; TypeScript and Core boundaries pass.
  The UI worker independently passed the full suite twice. Test-only Mantine environment
  setup removes a jsdom layout race without changing production menu timing or behavior.
  See `2026-09-07-ui-menu-status-regressions.md`.

Subsequent visual inspection found one more inconsistent footer: a red session dot paired
with a hidden driver-only "Ready" label. The footer and node now share session presentation;
the marker's color, accessible text, and tooltip derive from the same status. Root confirmed
hot reload shows "Session: clocks not synchronized" in both locations. No new persistent
explanatory text was added. During this check the live session's block size independently
changed from 128 to 256; no root or worker test issued that mutation. The latest user-owned
configuration was preserved rather than overwritten with the earlier backup.

After the footer correction, root reran the full final UI suite: 39 files / 316 tests pass,
TypeScript and Core boundaries pass, and the production build succeeds. The pre-existing
large-chunk warning remains (main JS 785.65 kB, gzip 242.18 kB); no bundle-size improvement
is claimed. Engine-service all-target strict Clippy, workspace formatting and whitespace
checks also pass. See `2026-09-07-footer-health-status.md`. Root DOM inspection confirms
the footer marker has `data-tone="warning"`, color `rgb(200, 165, 108)`, and the same
"Session: clocks not synchronized" accessible label as the nodes.

## Verification scope

The local macOS seven-package Rust run passed 168 tests with one opt-in ignored; strict
Clippy passed. This total includes platform-independent and native macOS tests but is not
multiplied into Windows/Linux claims. No changes were staged or committed; pre-existing
dirty/untracked work was retained. Supported-format state, native PCM, and OS-specific
acceptance evidence are kept separate rather than inferred from a green build.
