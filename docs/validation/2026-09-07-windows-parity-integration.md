# Windows shared implementation integration — 2026-09-07

This is a **Windows ARM64 build and ordinary-test qualification**, not a claim
that every native device/plug-in behavior is qualified on Windows.

## Scope

Authorized Windows VM `712A936E-9D91-4455-A8A9-A1F65EB60719`, Rust/Cargo 1.96.0,
`aarch64-pc-windows-msvc`, Visual Studio Build Tools ARM64. Tests ran without an
interactive audio session; every native/interactive ignored test stayed ignored.
No engine service was started, no VM restart/device/default/driver/security change
was made, and no application configuration or routing graph was used.

The stable snapshot comprised 57 files: root Cargo manifest/lock, the desktop
workspace manifest (metadata only), all six shared crates' source/manifests and
fixture, and engine-service source/build script/manifest. Vendor and UI source
were not modified by this validation. Guest SHA-256 values were checked against
the exact manifest for each synchronized snapshot, not inferred from a host pass.

The seven selected packages were `audio-io`, `audio-graph`, `audio-engine`,
`control-protocol`, `control-server`, `processor-host`, and `engine-service`, each
with the `tonenari-` prefix. The job runs `cargo build --locked ... --lib --bins`,
`cargo test --locked ... --lib --bins`, then `cargo clippy --locked ... --all-targets
-- -D warnings`.

## Runs

| Run | Source boundary | Build / tests / Clippy |
| --- | --- | --- |
| Earlier isolated device baseline | Before shared graph-catalog integration; three packages only | 76 passed, 7 ignored; Clippy passed. Not the same test scope as the runs below. |
| v1 | Initial stable shared snapshot, including Windows child-crash fixture | Build passed; 123 passed, 7 ignored; Clippy failed on `mutate_catalog` 112/100 lines. Failure log preserved. |
| v2 | Final graph-catalog helper refactor and nine catalog tests | Build passed; 123 passed, 7 ignored; Clippy passed, job exit 0; 57/57 guest hashes matched. |
| v3 | Final typed graph-catalog route contract test | Build passed; **124 passed, 7 ignored**; Clippy passed, job exit 0; **57/57** guest hashes matched. |
| v4 | Parameter durability, catalog concurrency/rollback, optional response value and off-thread parameter route | Build passed; **130 passed, 7 ignored**; Clippy passed, job exit 0; **57/57** guest hashes matched. |
| v5 | Frozen graph-layout persistence and virtual-source gain snapshot | Build passed; **143 passed, 7 ignored**; Clippy passed, job exit 0; **60/60** guest hashes matched before compilation. |

The v4 manifest preserves the executed snapshot. A subsequent host-only rustfmt
change collapses the Windows child-fixture `spawn().expect(...)` expression to
one line in `processor-host`; that formatting-only change is not in the tested
guest snapshot and is not represented as a new Windows execution.

## V5 graph layout and virtual-source gain

The complete frozen shared/service snapshot was synchronized again for v5,
including `graph_layout.rs`, `graph_layout_tests.rs`, and
`virtual_source_gain_tests.rs`. This also includes the formatting-only fixture
change previously excluded from v4. The host snapshot was checked again after
transfer and was unchanged; all 60 guest SHA-256 values matched before the job
was allowed to compile. Vendor and desktop implementation/UI files were not
part of this synchronization (desktop Cargo metadata only).

The Windows test counts are audio-engine 21, audio-graph 17, audio-io 16,
control-protocol 4, control-server 8, engine-service library 48 and binary 17,
processor-host 12: **143 passed**, with the same **7 ignored native tests**.
New coverage includes independent layout revisions, inactive graph layout across
rename/selection/restart, disk failure rollback, old-schema compatibility,
invalid positions/viewport rejection, typed layout routes, and virtual-source
gain persistence/validation/PCM scaling without extra audio-cycle allocations.
These are common implementation tests on Windows, not new native device or
desktop command-proxy execution evidence.

Read-only VM inspection before and after v5 found the exact LAB root `OK`,
`ProblemCode=0`, `ConfigFlags=0`, all eight endpoints `OK`, and no ToneNari
process. The previously recorded pending-restart bit remains unchanged. No
PnP operation, restart, scheduled task, security change, or native acceptance
run was performed for v5. The source manifest, guest hashes, output log, and
pre/postflight snapshots are retained as `windows-integration/v5-*` evidence;
guest job `C:\ToneNariLab\windows-parity-integration-v5` exited zero.

New shared behavior exercised in v2/v3 includes independent persisted graph
documents, switching/renaming/restart, graph/catalog concurrency revisions,
corrupt-file preservation, failed native staging, and persistence failure after
native preparation. Processor state tests exercise binary snapshots, bounded
capacity, timeout acknowledgement and concurrent parameter/state control. The
final route contract test verifies graph identity, both concurrency baselines,
and malformed catalog request rejection through the HTTP boundary.

The processor worker crash/reap test now executes on Windows using
`cmd.exe /d /c exit 1` (`/d` prevents AutoRun commands). It passed in the actual VM;
previously this test was Unix-only. This is real child-process lifetime evidence,
not a native audio or installed plug-in test.

## Evidence and remaining gates

Evidence directory:
`/Volumes/Linear 1/test/tonenari/evidence/native-device-parity-20260907/windows-integration/`.
Each run retains a source SHA-256 manifest, independent guest hashes, and the
complete build/test/Clippy log. Guest unique jobs remain under
`C:\ToneNariLab\windows-parity-integration-v1`, `-v2`, and `-v3`.

No native PnP, physical multichannel hardware, installed VST3/CLAP processing or
editor lifecycle, long-duration drift, desktop UI interaction, or x64 Windows
qualification is implied. After a separately authorized reboot, actual LAB PnP
acceptance still did not pass: this LAB parent requests restart for stop, and its
reserved disable state was independently restored using normal WMI enable; see
[the separate PnP record](2026-09-07-windows-device-pnp.md).
