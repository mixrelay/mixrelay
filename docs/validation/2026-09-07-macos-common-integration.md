# macOS common integration — 2026-09-07

This is focused integration acceptance, not a claim that all OS features or all
native audio paths are qualified. See the [feature matrix](../feature-parity-matrix.md).

## Tests executed by the coordinator

- Audio engine 19, graph 15, audio I/O 16, processor host 13: **63 passed**.
- Control protocol 2, control server 6, engine service library 37, macOS binary 2:
  **47 passed**, one live aggregate test ignored by this ordinary run. Its separate
  explicit native acceptance is recorded in [device parity](2026-09-07-native-device-parity.md).
- Desktop proxy 3 passed, including malformed catalog and same-active graph tap
  ownership. Desktop all-target Clippy with warnings denied passed.
- The seven common Rust packages passed all-target Clippy with warnings denied.
- UI production build/typecheck passed; the final integrated UI suite passed 97
  tests across 18 files, including the catalog-specific error message follow-up.
  Vite retained its existing large-chunk warning (765 kB minified JS); no bundle
  performance improvement is claimed.
- `bash scripts/check-macos-endpoint-cancellation.sh` passed ASan/UBSan and strict
  warnings: 1,000 cancelled preparations, 1,000 retirements with control readers,
  and all 3,004 mappings reclaimed. The retired-slot rejection test also checks
  that unpublished transports never become visible as active.

## Actual installed development engine

The old running service returned HTTP 404 for `/api/v1/graphs`. Before replacement,
the exact live graph (one duplex 2/2 endpoint and two explicit channel cables) was
backed up, together with the previous executable. Only the owned ToneNari engine
process was terminated; launchd restarted it with the rebuilt executable. The HAL
driver, coreaudiod, host default devices, and hardware rates were not changed.

Endpoint-only preferences did not contain cables. After the first restart the
backed-up complete document was applied through the normal graph API, preserving
all node/cable identifiers, settings, and routing. It was persisted into the new
catalog. A second engine restart restored that complete document automatically.
The coordinator compared the exact JSON structure, excluding the runtime revision,
and verified `activeRevision == submittedRevision == 2` after recovery. The new
catalog API returns HTTP 200, one existing graph, and no persistence warning.

`pnpm --dir apps/desktop tauri dev` was started and left running with Vite on
127.0.0.1:1420. The unbundled development process is running, but the native UI
automation inventory could not target it, so no visual/native pointer acceptance
is claimed. Requests briefly failed during the engine restart; the final HTTP
status and graph endpoints recovered.

**Residual:** live status still reports `clockAuthority=engine_timer` and
`issue=clock_not_synchronized`, with increasing underruns. Connectivity and exact
graph recall are verified; synchronized/underrun-free audio is not. The status
snapshot preserves these counters rather than masking them as healthy audio.
Follow-up [clock/demand inspection](2026-09-07-macos-clock-demand-audit.md) found
the HAL stopped with no active clients: idle timer reads were being counted as
short transfers. These growing counters are not evidence of audible playback
dropouts. Native clock/demand integration remains unimplemented; the interpretation
does not turn the synchronization gate into a pass.

Evidence and recoverable backups:
`/Volumes/Linear 1/test/tonenari/evidence/native-device-parity-20260907/macos-integration/`.
The graph backup, restore request, post-restart catalog/status, and old executable
are retained there. No source staging, commit, or branch operation was performed.
