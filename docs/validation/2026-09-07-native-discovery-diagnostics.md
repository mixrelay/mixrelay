# Native discovery failures are not empty catalogs — 2026-09-07

## Existing requirement repaired

An installed plug-in that cannot be scanned must expose a failure and a real retry,
not appear as "no installed effects". Previously format-child failure was converted
to `Vec::new()` and failed probes disappeared through `filter_map`. UI recovery for
transport failures could not detect those successful-looking responses.

- `ExternalProcessorCatalog` now carries optional `diagnostics` with format,
  scan/probe stage, optional plug-in ID and native reason. Empty diagnostics are
  omitted; old JSON still decodes. HTTP and Tauri preserve the same additive shape.
- Catalog entries and diagnostics publish as one snapshot. Rescans serialize while
  readers retain the previous snapshot; third-party children run outside its lock.
- Failed formats retain their previous entries. Failed probes retain only the
  exact format/ID/path candidate. Successful formats still refresh, successful
  empty scans remove previous entries, and a changed path cannot inherit a stale
  bundle. Duplicate locators are probed once. Successfully identified non-effects
  remain deliberately excluded from the audio-FX menu, not reported as failures.
- A cached GET containing diagnostics makes Retry execute a new scan. Scan errors
  preserve the known catalog and diagnostics until a successful replacement.
- The menu now distinguishes scanning, incomplete discovery and a clean empty
  result. Existing effects remain usable. Details are collapsed, scroll-contained
  and rendered as text, with English/Korean/pseudo copy and one explicit retry.

## Discovery child lifecycle

The previous pipe reader could remain blocked on EOF after the scanner exited if
a descendant inherited stdout. Discovery now captures into privately created files
without reader threads. Direct children are killed and reaped on unsuccessful
waits; owned files close and are removed on scope exit.

- Per-child deadlines: scan 45 seconds, probe 10 seconds.
- Output checks every 10 ms: stdout 4 MiB, stderr 16 KiB; bounded reads.
- Invalid JSON and invalid UTF-8 are rejected, not silently repaired. Actual exit
  status and bounded stderr are retained as diagnostic context.

These are failure-containment/correctness changes, not a throughput or latency
optimization claim. The inherited-stdout fixture uses a finite one-second child
and asserts the scanner result arrives in under 700 ms; a 50 ms timeout fixture
asserts termination within three seconds. These are test bounds, not production
latency measurements. No whole-application leak-free claim is made.

Limits remain: size checks can briefly overshoot on disk between polls, and this
is not a process-tree sandbox. Vendor descendants are not killed here; on Windows
an inherited descendant handle may delay file deletion. Total discovery duration
also scales with the number of candidates. Full vendor failure isolation stays
open in the resource/recovery gates.

## Validation scope

- UI: 171 tests / 30 files passed. New coverage includes cached partial-scan retry,
  retained diagnostics on transport failure, stale owner rejection, all three
  locale alerts, text-safe native messages, clean empty results and the actual
  workspace menu props. TypeScript/Vite and Core boundaries passed; existing large
  bundle warning remains.
- Engine controller: 48 tests passed; desktop proxy: 4 tests passed on macOS.
- Final macOS Rust boundary run: processor-host 23, control-protocol 5 and
  control-server 8 tests passed. All-targets Clippy for these crates plus the engine
  controller and desktop passed with warnings denied; formatting and diff checks
  passed. Targeted UI Biome checks also passed.
- Discovery fixtures include successful empty / failure / retry, partial retention,
  exact bundle identity, non-effect reclassification, duplicate candidates, atomic
  snapshots, invalid JSON/UTF-8, exit+stderr, deadline, inherited stdout, size bounds
  and owned file cleanup. The invalid UTF-8 test failed before the strict decoder
  fix because corrupt input was accepted as a replacement character.
- No actual installed third-party scan, Windows/Linux VM execution, service
  restart, device change or native UI acceptance was performed in this slice.
  Prior VM results qualify their prior snapshots, not these new child-capture paths.

### Subsequent synchronized VM qualification

The later application-metadata snapshots include these child-capture paths:
Ubuntu ARM64 `app-metadata-v3` passed 155 common tests (processor-host 22), and
Windows ARM64 `windows-app-metadata-v5` passed 158 (processor-host 20); both
builds and Clippy passed. This supersedes only the VM-execution limitation above.
Actual installed vendor scans and native UI acceptance remain unverified. See
[snapshot hashes and evidence](2026-09-07-application-metadata.md).

The broader agreed-behavior audit is in
[agreed-behavior-audit](2026-09-07-agreed-behavior-audit.md). Native vendor discovery
qualification remains open; this change does not close the whole OS feature row.
