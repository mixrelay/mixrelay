# Local controls and Stream Deck

## Implemented boundary

- The macOS desktop owns a same-user Unix socket, not an additional TCP listener.
  Typed node and recorder commands reuse the existing engine validation and the
  desktop's capture-aware preparation/publication boundary.
- Request admission closes first on app exit. Idle socket readers are cancelled,
  accepted socket work is joined, then native capture preparation and shared
  HTTP requests are drained before releasing the app's engine lease. A timeout
  retains ownership and reports failure instead of claiming clean shutdown.
- Explicit graph/node IDs, observed publication revisions, a per-desktop session,
  and observed recording state/path guard commands. No delayed/offline replay.
- Gain is exposed only for actual graph-local gain nodes, device inputs,
  application captures and virtual endpoints. Outputs without gain do not acquire
  a fictitious gain capability. Finite -60..12 dB validation precedes publication.
- External controls-only changes rebase the latest view and Undo/Redo/pending
  history, preserving layout. Audio/topology conflicts require explicit recovery.
  Definite rejected node commands follow this same reconciliation path.
- Stream Deck uses one generic node action. Its inspector lists live targets and
  supported operations; recorder operations are absent for other node types.
  Keys support power, mute, gain steps and recording transport; dials support
  gain and mute. OFF/deleted/unavailable targets do not invoke unsupported actions.
- New takes use unique filenames in the configured existing folder, follow input
  rate and use the same PCM24 default as the app. Resume retains take settings.
- Protocol and usage: `apps/streamdeck-plugin/README.md`.

## Executed checks

- Rust: 249 engine library, 10 protocol, 26 control server and 43 desktop tests
  passed. Includes gain rejection/no publication, previous-take stop rejection at
  the same graph revision, stale-session rejection, unknown command fields,
  socket permissions/live-path protection/stale recovery, idle-reader shutdown,
  and shared request drain before shutdown.
- UI: routing workspace, graph catalog and history suites passed (372 tests in
  31 files). Added controls-only reconciliation, history rebase,
  stale control conflict, and layout movement during refresh coverage.
- Stream Deck: 9 tests passed, including a bounded fragmented socket response,
  failed-write non-replay, successful 202 publication acceptance, missing/OFF
  targets, finite/clamped dial gain, take identity and the actual bundled official
  SDK registering/rendering against a simulated Stream Deck host.
- Official Stream Deck CLI validation and packaging passed with no warnings.
  Package: `apps/streamdeck-plugin/dist/io.mixrelay.streamdeck.streamDeckPlugin`.
- Rustfmt, strict Clippy for all targets of the four changed Rust packages,
  Rust documentation generation, engine debug build and Tauri app bundle build
  passed. UI TypeScript, Core dependency boundary and Biome checks passed.
  Plugin sources now use the same formatting/complexity gates. The pre-existing
  Vite >500 kB bundle advisory remains.
- Independent read-only review identified and rechecked fixes for shutdown
  admission, Undo loss after definite rejection and 202 acceptance handling.

## Runtime limits and preservation

### Unlocked-Mac follow-up

- After the user unlocked the Mac, the existing app window and all three graph
  identities were confirmed. All discovered recorders were stopped (one recorder
  in graph-1). Normal Cmd-Q closed the old desktop and port 19341, but the old
  engine process (PID 95746, launched before this implementation) remained alive.
- A process sample showed the main thread joining the processor reclaimer;
  native device workers were waiting inside CoreAudio `AudioDeviceDestroyIOProcID`
  and property reads, while reclamation was waiting on another CoreAudio property
  read. The log reported two quarantined device owners. This is evidence of an
  unfinished native teardown, not a successful server exit or proof of its root cause.
- SIGTERM was sent only to that exact old engine after confirming stopped
  recording. It eventually disappeared; this does not establish that the signal
  caused its exit. No SIGKILL, CoreAudio service restart, HAL driver reset or
  unrelated application restart was performed.
- The new desktop was launched normally from the debug app bundle. The private
  control directory and socket had permissions 0700 and 0600, respectively.
  An actual stale-session node request returned 409 `graph_revision_stale` with
  `nodeControlOutcome: not_applied`. User graph identities and the selected
  document revision were retained. Successful live mutation remains unverified.
- Native device initialization still timed out. While catalog/status reads
  responded, node discovery originally waited behind the restoration mutation
  lock. Node discovery, preview, change and both recorder boundaries now use
  nonblocking admission while retaining the same validation lock. A regression
  holds that lock and requires all five requests to reject before it is released.
  On the affected live host, discovery returned 409 `graph_exchange_busy` in
  1.662 ms and a local snapshot failed explicitly in 9 ms. These are diagnostic
  samples, not a controlled before/after performance benchmark or a DSP claim.
- The default macOS Quit menu called native `terminate:` and bypassed Tauri's
  preventable exit request. Replaced only that application submenu action with
  the same managed exit path as the tray, retaining standard submenu items.
  The revised build's Cmd-Q removed the socket and the engine exited with a
  `stopped` record. At the 40-second observation the desktop still remained;
  a second Cmd-Q then exited it. Port 19341 and both processes were absent at
  final inspection. This is not a clean one-step shutdown pass.
- Native in-flight CoreAudio teardown/recovery delays remain unresolved. The
  reclaimer can attempt recovery during teardown; a process sample located
  native property reads and device destruction in waits. Dock-native Quit,
  the actual tray action and healthy live control/reconnection remain unverified.
  No claim is made that all application termination paths are covered.
- After the fixes: engine library tests (249), desktop tests (43), Rustfmt,
  strict Clippy on both packages, documentation generation, engine build and
  rebuilt Tauri app passed. Independent read-only review found no blocker in
  the bounded changes. The app was left closed, without a running audio server.
- Neither `/Applications/Elgato Stream Deck.app` nor `/Applications/Stream Deck.app`
  was present. Actual host installation, key/dial interaction, live PCM effects
  and reconnection across real app restart remain unverified. Simulated SDK tests
  are not claimed as hardware validation. No hardware app was downloaded or installed.
- No user graph, workspace, recording or branding source was removed. No commit
  or push was made for this implementation. Existing user artwork remains untouched.
- This is functionality/lifecycle safety work, not a DSP performance change.
  No real-time processing code was changed and no speedup is claimed; no
  performance comparison or full device-dependent native fixture run was made.

### Native recovery and shutdown correction

The subsequent fix changes the ownership boundaries behind the delays above:

- Both execution composition and native device recovery now pass enabled device
  selectors to one shared macOS identity resolver. It resolves explicit UIDs and
  requested default aliases without enumerating unrelated device geometry.
  Explicit process-private selections remain supported; defaults never silently
  substitute missing devices. The two other OS adapters received only the required
  trait signature argument, with no porting or native behavior change.
- The reclaimer only retires resources. Device observation/recovery belongs to
  the stoppable format monitor; shutdown cancels periodic observers before HTTP
  drain, then joins observation before the final recording/editor checkpoint.
- Device startup/teardown timeouts no longer detach untracked workers. Their
  retained join handles are reclaimed outside DSP and drained after all graph
  producers drop, before recording server exit. Worker panics remain failures even
  when periodic reclamation observed them before final shutdown.
- The desktop's 30-second threshold is a one-time pending notice, not abandonment
  of the quit request. A virtual-time test waits 65 seconds for a native exit and
  finishes the original request, without another click or a false success.

Validation on the corrected source:

- 329 tests passed: 15 macOS audio-I/O, 249 engine library, 9 maintenance,
  3 worker retirement, 8 device recovery, and 45 desktop tests.
- Strict all-target Clippy for audio-I/O, engine and desktop, Rustfmt check,
  documentation generation for the three packages, engine build and normal debug
  Tauri app bundle build passed. Existing Vite chunk-size advisory remains.
- Independent read-only review rechecked the actual composition call path,
  CF string ownership/UID qualifier boundary, worker-error propagation and
  retirement ordering. No new DSP processing, dependency stack replacement,
  system-service reset, force kill, or instruction-file staging was introduced.
- Same-fixture query-count comparison: with 100 device descriptors and one
  vanished ID, full enumeration invoked 101 device property probes; selection
  of two identities plus default aliases invoked 2 probes (99 fewer, about 98%).
  This is an operation-count regression, not a wall-clock or DSP benchmark.
  The earlier HAL stall did not reproduce on the final baseline launch; both
  healthy baseline and corrected Cmd-Q were gone at the first post-action poll.
  Therefore no precise end-to-end shutdown speedup is claimed from those runs.
- Normal corrected app launch restored all three user graph IDs and native
  microphone/headphone routing. The local control snapshot responded in 9 ms.
  The stopped recorder was switched OFF then ON through local IPC, producing
  revisions 342 and 343; the engine confirmed active/submitted 343. Recording
  remained stopped throughout and the original enabled state was restored.
- One Cmd-Q closed desktop PID 24632 and engine PID 24644, removed the socket,
  released port 19341 and recorded `stopped`. Native logs show desktop ownership
  released at 01:57:00.338884Z and both device connections retired by
  01:57:00.368035Z. This 29.151 ms interval covers those log events, not all app exit.
- A second normal launch restored the same graph/node states and issued a new
  local-control session. The user clicked tray Quit after automation could not
  access that menu. Desktop PID 24990 and engine PID 24997 were then absent,
  port 19341 was released, the socket was removed and the final record was
  `stopped`. Dock-native Quit remains unverified.
- Arbitrarily hung synchronous HAL RPCs cannot safely be cancelled in Rust.
  The app now retains and waits for those owners rather than detaching them or
  reporting success. The prior server-side stall's exact cause was not proven;
  no claim is made that unrelated CoreAudio or vendor failures are eliminated.

### Dock dismissal and explicit application exit

- Added `applicationShouldTerminate:` to the existing Tauri delegate, only when
  neither that class nor an ancestor already implements it. Per the clarified
  product requirement, native Dock dismissal cancels termination and hides only
  the main window and Dock presence. It shares the window-close visibility
  helper and leaves the tray, socket, captures and audio server running.
  Explicit tray/application-menu Quit instead requests managed exit and drains
  socket admission, captures and the server before releasing the app.
  The retained delegate is reassigned to refresh AppKit's optional-method cache;
  the delegate itself and its other handlers are not replaced.
- The Objective-C boundary uses a typed callback and checks the termination
  reply ABI. Tests cover invocation and refusal to replace existing or inherited
  handlers. App-handle ownership is released on both build failure and final
  exit, outside the storage mutex.
- On the final source, Rustfmt and strict all-target Clippy passed for desktop,
  engine, protocol, control server and audio-I/O. Protocol/control-server tests,
  all 46 desktop tests and the normal debug app bundle build passed. Following
  the visibility correction, desktop formatting, strict Clippy, all 46 tests
  and documentation generation passed again.
- UI checks passed: 372 scoped tests, boundary validation, TypeScript and Biome.
  All 9 Stream Deck adapter tests and its build passed. The existing Vite
  chunk-size advisory remains; actual Stream Deck host/hardware operation is
  still not verified.
- The preceding bundle launched desktop PID 29994 and engine PID 30007. Local IPC
  restored all three graph IDs at revision 347, the original node power/mute
  values, and the recorder remained stopped with no output file or frames.
- AppKit does not identify the native termination request source in this
  delegate method. Logout/restart requests would also be cancelled by this
  visibility policy; OS session termination is not implemented or validated
  here. Force quit cannot run the managed drain and is not a clean-exit claim.
- The rebuilt visibility-corrected bundle launched desktop PID 31820 and engine
  PID 31825. After the user performed the requested dismissal, accessibility
  reported `noWindowsAvailable`; both exact PIDs remained alive, port 19341
  remained served by PID 31825, and the exit record remained `running`.
  Local IPC returned the same session and revision 351 with unchanged graph,
  power/mute and stopped-recorder state. No recording or restart was introduced.
  Independent read-only review confirmed dismissal touches only visibility,
  while explicit tray Quit still enters managed shutdown.
- The user then selected tray Open. Accessibility and a fresh screenshot
  confirmed the main graph window was restored, with live node meters and the
  recorder still stopped. The final visibility-corrected normal Tauri bundle
  build passed. The app was left running for use; no final forced stop was made.
