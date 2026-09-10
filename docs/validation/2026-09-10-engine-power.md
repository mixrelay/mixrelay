# Engine execution switch

## Contract

- Bottom-right power control retires/resumes execution through local control IPC;
  no administrator prompt, launchd modification, driver uninstall, or graph deletion.
- Authored catalog, selection, node configuration, edges and layout survive. Off
  composes an empty runtime, including graph edits and capture previews while off.
- Stop finalizes recorders and checkpoints plug-in state. Checkpoint/storage failure
  retains diagnostics but does not prevent publishing the stopped runtime.
- Start preflights/reserves desktop capture before publication, with revision CAS.
  Definitive rejection releases reservations; uncertain delivery uses the existing
  acknowledgement monitor. Native capture retirement also contributes to UI state.
- Off is not reported complete until the empty revision activates and macOS native
  device/group admission counters reach zero (including quarantined owners).
- The control service remains reachable. Power state is session-local, not a
  change to service installation or an application-launch preference.

## Verification

2026-09-10, local macOS debug build:

- Engine library: 230 tests passed; control server 7; protocol 17; process tap 23.
- Desktop capture proxy: 4 focused tests passed, including definite/uncertain
  power-start rejection. UI power/status/screen/app: 29 focused tests passed.
- Targeted strict Clippy (engine, desktop, server, protocol, tap, all targets),
  rustfmt check, UI typecheck, changed-file Biome and UI boundary check passed.
- Engine binary and debug `.app` built. Vite retains its existing large-chunk warning.
- Running service upgraded using its same-user graceful termination/restart; no
  administrator authentication was used. New engine PID 96287.
- App accessibility exposed the bottom-right `엔진 끄기` toggle, then `엔진 켜기`.
  The user interacted during live verification; automation refreshed rather than
  replaying a stale click. Native logs confirm on and off publications.
- At 20:08:22.126 KST: start accepted; direct hardware rendering resumed at .129.
  Stop accepted at 20:08:22.739; output retired at .811, input at .825.
  These are observed lifecycle timestamps, not a benchmark or performance claim.
- Subsequent status: `enginePower={enabled:false,transitioning:false}` and
  `processingState=stopped`. Thus native admission counts were zero.
- Before/after comparison confirmed all 3 graphs' node/edge/layout content and
  selected graph unchanged (execution revisions intentionally advance).
- Initially observed Off for the user's Parsec comparison; a later live status
  showed On again while the user was interacting. No further forced toggle was
  performed. Parsec audible recovery is not established by status or visual tests.

## Remaining live coverage

The existing microphone/headphone graph was exercised. A real muted application
capture, active recording, failing native teardown, unavailable plug-in and
off-mode workspace import were not all reproduced interactively in this run;
covered logic tests are not represented as those physical integration tests.
Windows/Linux runtime validation is outside this macOS task.

## Follow-up sidebar tooltip

Removed the `New graph` shortcut tooltip from the sidebar, preserving the keyboard
shortcut and exposing it through `aria-keyshortcuts` instead. A 500ms hover regression
check asserts no tooltip appears. The 4 StudioScreen tests and debug app build passed.
This follow-up only restarts the desktop UI, not engine execution.
