# Workspace files

User files use `.mix`, with a `MixRelay` format marker and explicit version 1.
The default location is `~/mixrelay`; automatic recovery remains in OS application
data. Pre-release formats are not migrated. Unsupported versions are rejected.

Save snapshots every running graph's plug-in state and stores graph/node/device
identities, connections, layouts and last-known plug-in channel contracts. Missing
plug-ins retain their settings and ports as silent nodes. Live worker export errors
are not silently replaced by old presets. Writes use the existing same-directory
temporary file, file sync and atomic replacement implementation.

Open/New validate before publication, preserve active recording ownership by
requiring Stop, and advance audio/layout generations. File recall does not merge
previous live presets or device settings. Device rate changes are acknowledged,
rolled back on preparation failure, and published together with the corresponding
DSP format and native resource set. Failed rollback leaves processing suspended.

On macOS, a read-only preview prepares canonical application captures before
replacement. SHA-256 binds the subsequent Open to the exact previewed file bytes.
Explicit unpublished failures release capture reservations; uncertain delivery
retains them until reconciliation. Accepted publication with a persistence warning
remains an accepted result, not a false rejection.

## Validation

- UI suite: 693 passing tests; type check and UI boundaries passed.
- Related Rust libraries: audio graph 26, protocol 7, server 17, engine 225,
  processor host 61 tests passed before the final preview addition.
- Latest workspace-specific tests: 7 passed, including shared-device/missing-plugin
  round trip, clock adoption/rollback, immutable preview and changed-file rejection.
- macOS device coordinator tests: 13 passed; desktop capture response tests: 3 passed.
- Strict workspace Clippy passed; engine and debug app builds succeeded.
- Full macOS binary suite initially had 135/143 passing. Seven tests require an
  explicitly selected `MIXRELAY_TEST_ENDPOINT_UID`, which was not supplied. The
  remaining format-reservation assertion was updated to the new activation contract
  and passed in the subsequent 13-test coordinator run. No full native pass claimed.
- Vite reports the existing large-chunk advisory; thresholds were not weakened.
- Local native Save created `~/mixrelay/Workspace-20260910-1735.mix`: 3 graphs,
  4 total nodes and 4 edges. New created an empty workspace; API Open restored
  the saved 3 graphs. Native Open's attached confirmation was verified; the complete
  Open UI flow remains unverified after user interaction interrupted the check.
- Local rebuilt app visually verified: sidebar heading removed; app and graph
  headers share a 32px height (previously 44px and 56px), with 13px titles.
  Final header change passed type checking, Biome, App's 3 tests and debug app build.

No performance improvement is claimed. No commit or push was performed.

## UI consolidation follow-up

- One 32px toolbar replaces the two stacked 32px rows. Brand/navigation and
  graph title/file actions share that row; normal Active text and dot are absent.
- Sidebar width 248px -> 200px; rows 36px -> 30px. Selection uses a background
  without a box border. New graph follows the list and is left aligned.
- Sidebar collapse expands the canvas to the full width without remounting the
  graph. Small-window navigation uses an overlay with grid placement reset.
- Shared node insets, settings gaps, surface colors and reduced shadows; canvas
  grid contrast reduced. Node dimensions, saved layouts and audio logic unchanged.
- UI: 694 tests, type check, Biome and boundary checks passed; debug app rebuilt.
  Local app verified single toolbar, normal-status removal, list placement,
  sidebar collapse/full-width canvas and node settings expansion/collapse.
- File menu DOM items are covered by the test. JSDOM has no anchor geometry;
  this does not prove popup visibility. Native automated File activation did not
  expose the menu, so native popup interaction and small-window visuals remain
  unverified. No speculative production hideDetached override was retained.
- Independent read-only review found one small-window grid offset issue, fixed
  with grid-area:auto; subsequent review found no additional code blockers.

Rust source and engine were not changed for this UI follow-up; Rust test/Clippy
suites were not repeated. No runtime performance improvement is claimed.

### Sidebar boundary refinement

Removed the top-left sidebar icon. The wordmark stays at the same 16px left
inset, including when collapsed. A 10px-wide boundary target below the toolbar
toggles the sidebar; after collapse it remains at the left canvas edge.
App composition tests (4), type check, Biome, UI boundaries and debug app build
passed. Local native clicks verified collapse and reopen; screenshot confirmed
the persistent wordmark and expanded canvas. Independent review found no blocker.
