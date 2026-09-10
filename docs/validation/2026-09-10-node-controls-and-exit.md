# Node power, mute, and application-owned engine lifecycle

## Contract

Node state belongs to the authored graph (`control.enabled`, `control.muted`), not
to a separate hardware-control registry. Missing control fields default to ON and
unmuted. UI and HTTP commands enter the existing serialized catalog/graph
transaction; native resource preparation and capture reservation retain their
existing ownership boundaries.

| Node | OFF | MUTE |
| --- | --- | --- |
| Input / application capture | No execution reference; release native owner only after its final graph reference disappears | Keep capture active; silence this graph-local source |
| Processor / gain / mixer | No kernel or hosted instance execution; block signal, never bypass | Continue processing and state progression; silence its output |
| Output | Remove this output reference; preserve other graphs using the same device | Silence this reference before the shared mix |
| Recorder | Finalize the take and stop capture; ON does not restart recording | Continue recording, writing silence at the input boundary |

Internal device buses and native transport nodes are not exposed as user controls.
The node Options row contains a 22x12px pill power switch in a 26x24px hit area,
vertically centered with the disclosure. Closed rows reserve 12px at the right
edge for the 10px corner resize grip; expanded rows use 6px right padding.
Mute sits beside the gain slider, or beside the meters when no gain control exists.
Compact plug-ins place mute in their level-control area. Power and mute remain
independent of disabled gain/parameter controls. Recorder power uses its own Options
row. Nodes without settings use a power-only row; generic plug-in parameters stay
visible without a disclosure. Commands share the current graph writer and capability
discovery. No hardware plug-in or external settings window is included.
Power uses a low-saturation green. Base cables use gray-teal at 1.5px and selected
cables use a lighter 2px stroke. These are base/selection colors only; the current
UI does not yet expose PCM-conversion classification on cables.
Each canvas permits one open Options disclosure across regular and recorder nodes.
Opening another disclosure closes the previous one; a blank-pane click closes it.
The recorder uses an explicit face/control grid so extra height belongs to the
meter face, not a blank footer below Options. The toolbar omits application-state
badges while retaining busy guards, error alerts, and recovery actions.
OFF preserves authored connections, parameters, and geometry. Shared device/source
owners are composed from remaining enabled references. MUTE does not reset
surviving unchanged compressor/limiter envelopes or shared bus snapshot history.
Those state transfers occur only for matching identity, geometry/configuration,
and render format, without allocation or locks on the audio thread.

## Command safety and API

- `GET /api/v1/graphs/{graphId}/nodes/controls` returns current graph-scoped
  targets, their control states, supported capabilities, and publication revision.
  No recorder node means no recorder capability to expose to hardware.
- `PUT /api/v1/graphs/{graphId}/nodes/{nodeId}/control` accepts
  `{ "expectedRevision": 7, "enabled": false }` or a `muted` field (or both).
  Fields are explicit state assignments, never a retry-sensitive toggle.
- `POST .../control/preview` uses the identical target and request for the existing
  desktop application-capture reservation path; it does not mutate the graph.
- Parameter, editor-open/close, and recorder writes require the revision observed
  when the intent was created. The recorder transport wraps its existing request
  in `{ "expectedRevision": 7, "request": { ... } }`.
- Missing nodes return `node_not_found`; inactive processing commands return
  `node_disabled`; stale revisions return `graph_revision_stale`; an unactivated
  publication returns `graph_exchange_busy`. None re-enables or recreates a node.
- Recorder Stop is idempotent when already stopped. Power and mute remain saved
  controls, including while processing is OFF. Engine OFF never becomes ON as a
  side effect of a node command.
- A deleted and re-added same-ID node cannot receive commands carrying the old
  revision. Native editor pointer input also retains its existing instance/session
  fence. Clients must not fetch a newer revision simply to retry an old intent.
- Definite node-control rejection carries `nodeControlOutcome: "not_applied"`,
  allowing provisional native captures to be released. Lost responses remain
  uncertain and are reconciled rather than assumed rejected.

Unavailable plug-ins retain the authoritative saved preset even if a stale UI
graph is submitted. A failed checkpoint during OFF permits stopping processing
but retains the last saved state and exposes a warning after publication/storage.

## macOS lifetime

Closing the main window hides it to the tray and keeps desktop ownership. Quit
from the tray, application menu, or keyboard releases the desktop lease and waits
for the engine lease and HTTP listener to disappear. Engine ownership is acquired
before checking desktop liveness, and retained through recorder/checkpoint/audio
teardown. A failed or empty finalization record is not reported as clean exit.

The HAL fences reconnects while desktop ownership is absent. launchd registration
remains available for demand start, with `KeepAlive=false` and `RunAtLoad=false`.
OS-released file locks, not stale PID files, represent process lifetime. The
installer provisions root-owned containing directories and rejects symlink paths.
No administrator action is needed for normal app start/quit.

The existing installation requires one administrator-approved update of the HAL
and launchd configuration. That update reloads CoreAudio and temporarily interrupts
system audio. Source tests do not prove this live migration has occurred.

## Verification

Native installed-app verification is separate from isolated tests and must not be
inferred from a successful build.

### Processing measurement

Measured serially on Apple M2 Pro / 32 GiB, `aarch64-apple-darwin`, Rust 1.96.0.
Repository release profile unchanged; neither `RUSTFLAGS` nor
`CARGO_ENCODED_RUSTFLAGS` was set. No other build/test/benchmark ran alongside the
measurement. Existing desktop/system audio remained in its normal background state.

Command: `cargo build -p mixrelay-audio-engine --release --example node_control_cost`,
then `./target/release/examples/node_control_cost`.

Same authored graph: 48 kHz, 256 frames, stereo, four compressors plus four
limiters. Identical input PCM, 100-block warmup excluded, 10,000 blocks per round,
nine rounds per state, state order rotated. Plan preparation is outside timing.

| State | Median ns/block | Sorted nine samples (ns/block) |
| --- | ---: | --- |
| ON | 19,127 | 18312, 18322, 18375, 18597, 19127, 19404, 19573, 19766, 20256 |
| OFF | 412 | 337, 354, 355, 360, 412, 428, 495, 498, 555 |
| MUTE | 17,087 | 16716, 16742, 16773, 16818, 17087, 17210, 17390, 17530, 17651 |

OFF reduces this fixture's block-processing time by 97.8% versus ON. This is a
same-build ON-to-OFF comparison, **not** an application-wide CPU claim or a
code-version benchmark. A pristine pre-task source baseline was not captured;
the starting worktree already contained independent unpublished engine changes,
so an older commit would not isolate this change. The alternative structural
regression measures 100/0/100 hosted processing calls for ON/OFF/MUTE over the
same 100 blocks, and checks zero audio-thread allocations in all three states.
MUTE can have different arithmetic cost because downstream processors receive
silence, but their processing calls and state progression are retained.

### HTTP shutdown stalls

A real TCP peer that stopped reading a 32 MiB response held graceful shutdown
beyond three seconds before the transport fix. After the bounded listener change,
the full control-server suite passes, including incomplete-body and non-reading
peer shutdown tests. Request-body collection is limited to two seconds / 16 MiB;
socket write stalls are limited to two seconds. Neither deadline runs while an
accepted mutation is executing. A separate real TCP test confirms a command
taking longer than two seconds still completes during graceful shutdown.

### Executed checks

- `cargo fmt --all --check`: passed.
- Strict `cargo clippy ... --all-targets -- -D warnings`: passed for audio-engine,
  audio-graph, engine-service, control-protocol, control-server, macos-transport,
  hal-core, and desktop. No lint thresholds were weakened.
- Frozen library suites: audio-engine 44, audio-graph 27, engine-service 241,
  control-protocol 10, control-server 26, macos-transport 139, hal-core 50 passed.
- Desktop unit tests: 37 passed.
- UI changed-boundary suite: 15 files / 166 tests passed. Type checking, production
  UI build, and UI dependency-boundary check passed. Vite retains the existing
  large-chunk advisory (854.46 kB minified JS); it was not hidden or raised away.
- Embedded-assets desktop binary build (`cargo build -p mixrelay-desktop
  --features tauri/custom-protocol`) and release engine build passed. These builds
  did not replace the currently running bundled desktop or debug engine service.
- Installer shell syntax and `git diff --check`: passed.

The full macOS engine-binary suite initially returned 136 passed / 7 failed.
One failure was a fixture that activated the DSP plan but not its newly published
device-resource handoff. It now uses the production configured-activation boundary;
the complete rate/buffer test passes on re-run, including pending-resource drainage.
The other six tests require explicit installed native test device UIDs
(`MIXRELAY_TEST_ENDPOINT_UID` / aggregate primary-output fixture). They failed before
exercising their device contract because no such fixtures were selected. They were
not disabled, rewritten as mocks, or counted as passing. The full installed-device
suite remains unverified. Quit/reopen/audio behavior was unverified at this stage;
the later installed validation below records what was subsequently observed.

### Deployment boundary

Before the installation pass, the installed launchd plist still had
`KeepAlive=true`, `RunAtLoad=true`. The updated desktop rejects that configuration
instead of silently reverting to always-running behavior.

Independent read-only reviews covered node state/resource composition, saved
plug-in preservation, realtime state transfer, lifecycle admission/teardown,
installer paths, and bounded HTTP transport. Findings were fixed and re-reviewed.
The UI also recognizes definitive `not_applied` failures independently of HTTP
status: with no pending draft, it refreshes authoritative state and leaves other
controls usable. Uncertain replies or a pending draft retain explicit recovery;
the failed node intent is never automatically replayed.

## Installed validation, 2026-09-10 21:48 KST onward

- User approved installation. The existing installer rebuilt the arm64/x86_64 HAL,
  passed its factory/COM probe and strict signature verification, updated the HAL
  and launchd registration with administrator authentication, and reloaded CoreAudio.
- Installed `KeepAlive=false`, `RunAtLoad=false`; lifecycle directories are
  root:wheel 0755 and fixed lease files are state303:wheel 0644.
- Before opening the app: no desktop/engine processes, launchd `not running`,
  `runs=0`. Opening the normal bundled app started desktop PID 45068 and engine
  PID 45073 via Mach demand start; launchd `runs=1`.
- Initial connection warning recovered automatically. Saved three-graph contents
  (names, IDs, nodes, parameters, and edges) matched the pre-install backup after
  normalizing default control fields. Ready status: 48 kHz / 256 frames, zero
  underruns/overruns. The backup is `/tmp/mixrelay-install-oVwUzmEq/graphs.json`.
- Clicking the main window close control changed application type to `UIElement`
  while both PIDs remained alive. Reopening restored the graph and controls.
- Keyboard Quit released desktop ownership at 12:53:58.800 UTC. Both native audio
  connections retired by 12:53:58.838. Desktop and engine processes disappeared,
  the port 19341 listener disappeared, the exit record was `stopped`, and launchd
  reported `not running`, `last exit code=0`, `runs=1`. Later checks during the UI
  rebuild still showed no processes, so no automatic restart occurred.
- The actual tray Quit click has not been independently observed in this pass;
  it uses the same reviewed exit handler. Active-take finalization and the six
  explicitly selected native-device fixture tests are not claimed as live-tested.

### Sidebar follow-up

The New Graph action is a native 28px list row, removing the component button's
minimum-height interaction. A collapsed sidebar exposes a 20x28px chevron tab
without shifting the logo. The canvas add-node control shifts to x=32 while
collapsed, leaving a 12px horizontal gap from the tab even when warning banners
move the canvas vertically. Expanded divider interaction and keyboard controls
remain. Studio/App/canvas focused tests: 25 passed; UI boundaries passed.
Independent read-only review closed both normal and warning-banner overlap cases.
The updated normal bundle was reopened (desktop PID 50156, engine PID 50164;
launchd runs=2 only after explicit app launch). Ready status remained 48 kHz /
256 frames with zero underruns/overruns. Native UI screenshots confirmed the small
New Graph row and visible collapsed chevron, and clicking that chevron restored
the three-item list. The app was left open with the graph list expanded.

### Node power and mute UI verification

- Changed-boundary UI/API tests: 10 files / 101 tests passed. Biome, type checking,
  UI boundaries, `cargo fmt --all --check`, desktop strict Clippy, and 37 desktop
  tests passed. Normal debug app bundle built successfully; Vite's large-chunk
  advisory remains (856.44 kB minified JS).
- Native app screenshot verified pill power switches centered in Options rows,
  microphone mute beside its gain slider, and recorder/output mute beside meters.
  All switches keep clearance from the resize grip.
- The recorder pill switched OFF to ON and back to OFF, with Record becoming
  enabled then disabled. Its disclosure stayed folded; no recording was started.
  The microphone Options panel was left open to show the gain/mute arrangement.
- Meter mute positioning uses a separate measured face height without changing
  minimum geometry. Independent source review checked the resize calculation;
  a live tall-node resize was not exercised in this pass.
- Before reopening the new bundle, keyboard Quit left no desktop/engine process
  or port 19341 listener. The new app was left running with the original node
  power and mute values restored.

### Final UI pass

- Changed-boundary tests: 11 files / 107 tests passed; the final test typing cleanup
  was followed by another 3-file / 33-test run. Type checking, Biome, UI boundaries,
  and diff whitespace checks passed. Desktop fmt, strict Clippy, and 37 desktop
  tests passed; no Rust source changed. The final normal debug app bundle built
  successfully (856.38 kB minified JS; existing chunk-size advisory remains).
- Native screenshots confirmed the recorder footer no longer leaves a blank strip
  below its closed Options row. A WebKit fixture also checked the face/control grid;
  the earlier flex-layout gap was visible in the installed app, not that fixture.
- In the final app, opening microphone Options then recorder Options collapsed the
  microphone panel. Clicking the empty canvas collapsed recorder Options. No gain,
  power, mute, or recording command was sent during this check. The app remains open
  with both disclosures closed, recording stopped, and buffer size 32.
- Normal Quit before reopening left no desktop/engine process or port 19341 listener.
  Temporary layout-probe source files were removed and are not part of the commit.
