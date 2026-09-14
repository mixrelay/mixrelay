# Incremental node controls

## Contract

- Power, mute and gain publish a coherent batch into the existing render plan.
  The audio callback samples once at the block boundary without allocation,
  locks or retry loops. Unchanged controls do not scan the control bank.
- OFF skips the node kernel, external transfers and recorder capture. It is not
  bypass. Mute preserves processing state while gating its signal.
- Shared native owners follow the OR of graph-local enabled references. Gain
  and mute remain local; a virtual endpoint's input gain is not applied twice
  to its independent output path.
- Topology identity and global publication revision are separate. Native
  attachments, meters and processor timing retain the topology identity; clients
  use the global revision for compare-and-set and audio acknowledgement.
- An initially unprepared provider/device needs explicit preparation on first
  activation. Its acknowledgement requests a canonical graph read because that
  preparation can also reconcile device or provider metadata. Ordinary control
  acknowledgements are applied locally without a graph read or resubmission.
- Read-only discovery uses a committed immutable control view, including stable
  recorder identities. It does not enter graph mutation. Writers use bounded
  admission, then revalidate the revision; stale/deleted commands are not replayed.
- Internal topology edits with unchanged external geometry and format can swap
  at the existing macOS callback. Actual device membership, format or binding
  changes still require native preparation. This is not a claim that every
  topology edit is cost-free or that OFF destroys shared native resources.
- Non-direct physical device OFF/ON uses per-direction request/acknowledgement.
  The worker resets its converter and owned source cursor before graph transfers
  resume; native output is gated while reset is unacknowledged. FIFO reset does
  not change the hardware pacing epoch or reset unrelated graph timelines.

## API

- `GET /api/v1/controls/snapshot` returns publication/active revisions, topology
  generation, engine power and graph/node controls with recorder status in one
  request. Desktop adds its local session identity before forwarding to adapters.
- Node previews return an optional capture manifest. Gain/mute need no capture
  reservation. Application power edits retain the existing capture transaction.
- Node acknowledgements include required `requiresGraphReload`: false for an
  in-place/no-op edit, true for explicit structural preparation. This is a node
  control response, not a second graph update protocol.

## Measurement method

The pre-change fixture used 64 nodes, 100 alternating mute edits, 48 kHz and a
128-frame plan, including preview, command validation and durable catalog writes.
It called the processor factory **100 times**. The debug run took 2,136,858 us.

The revised fixture additionally processes one 128-frame block per edit to
verify the new audio acknowledgement boundary. Therefore elapsed times are not
an identical-workload comparison and must not be reported as a speedup. Factory
and compiler invocation counts for the same 100 edits remain directly comparable.
The changed fixture reports **0 factory preparations and 0 compiler invocations**
for those 100 edits, with the same topology revision throughout: 100 preparations
eliminated (100%). The standalone debug run took 1,926,261 us, but the additional
audio-block processing means that duration is diagnostic only, not a speedup.
Durable disk writes are deliberately still included and not bypassed.

## Validation status

- Audio engine 57, audio graph 31, processor host 61 and service library 258 tests
  passed. Includes coherent batch races, actual recorder PCM across 100 control
  edits, retained provider/dynamics state, shared capture manifests and first
  activation of unavailable OFF nodes. These are automated fixtures, not proof of
  the user's live device or Stream Deck interaction.
- Strict all-target Clippy passed for audio engine, graph and processor host.
- Protocol 13, control server 27, desktop 48 and macOS adapter 58 scoped tests
  passed, bringing the affected Rust total to 553. The macOS fixture alternates
  100 internal node additions/removals across 101 PCM callbacks: the renderer and
  frame epoch stay unchanged, with zero reported underruns/overruns. Non-direct
  OFF/ON fixtures verify old PCM is discarded before fresh PCM is admitted and
  another device's queue and hardware epoch are unchanged.
- Strict all-target Clippy also passed for service, desktop, protocol and control
  server. Whole-workspace Rustfmt check and seven-package documentation generation
  passed. No new dependency or instruction-file staging was introduced.
- UI focused tests 46, TypeScript, dependency-boundary checks and 15-file Biome
  validation passed. Stream Deck's 14 tests, build, official CLI validation and packaging
  passed. Tests include zero graph reads for ordinary UI control acceptance and
  one canonical read when resource preparation reconciles metadata.
- Engine and desktop debug binaries and the production UI build passed. The
  existing Vite >500 kB chunk advisory remains. Installed verification of this
  stage is recorded separately below; later follow-ups need fresh validation.

## Remaining runtime boundaries

- The follow-up below stops the macOS virtual boundary SRC while its canonical
  direction is OFF. This does not claim zero native callback CPU or release a
  device still referenced by another graph.
- Full hardware-dependent fixture suites, Windows/Linux native execution and
  the user's live PCM/Stream Deck continuity are not covered by the automated
  results above.
- The running app's recorder was confirmed stopped before planned replacement.
  The initial locked-Mac attempt did not replace the app; after unlock, normal
  Cmd-Q removed desktop PID 31820 and engine PID 31825 and released port 19341.
  A normal debug app bundle was built and launched, preserving the current graph.
  No icon regeneration, force kill, system audio reset or user-graph restoration
  from an earlier snapshot was performed.

## Installed control verification

- Stream Deck ignores a same-version package as `AlreadyInstalled`. Incremented
  its build version to 0.1.0.1 and installed through the normal confirmation UI.
  Installed and built plugin.js SHA-256 both match
  `234f5fe5961207d619967838c6e21abc09b0d7f0b324cd1111ac4bfa722cd18d`.
- At 48 kHz / 32 frames, ten alternating headphone OFF/ON commands used the same
  desktop local-control socket as Stream Deck. Each observed audio acknowledgement
  succeeded; revisions advanced 392 through 401 while structure generation stayed
  391. Final node states matched the initial states and recording stayed stopped.
- During that controlled command window: native connection starts/retirements 0/0,
  renderer starts/stops 0/0, mutation errors 0. End-to-end acknowledgement samples
  were 20.3–25.6 ms including local IPC, persistence and snapshot observation.
  This is a live after-only diagnostic, not a same-condition elapsed-time speedup.
- The app's actual headphone button was then clicked OFF and ON; its accessible
  state and label changed correctly both times. The user then confirmed that
  actual Stream Deck Mobile headphone OFF/ON had no problems. Their subsequent
  headphone OFF setting was left untouched.

## Virtual endpoint SRC follow-up

The user explicitly added the remaining virtual endpoint cost to this work.
The deterministic before fixture runs 1,000 native callbacks at 48 kHz / 32 frames
against a 44.1 kHz duplex virtual device. In both ON and OFF cases it measured:

- source begin/read: 1,000 / 1,000;
- sink begin/push/flush: 1,000 / 1,000 / 1,000;
- transport reads/writes: 999 / 250.

Counters are at the actual converter call sites under `cfg(test)`; production
control flow was unchanged for the before run. The identical after fixture now
reports OFF source begin/read 0/0, sink begin/push/flush 0/0/0, and transport
reads/writes 0/0: all measured OFF conversion/transfer calls eliminated (100%).
ON retains the exact before counts. This is operation-count evidence, not a
wall-time or overall CPU percentage claim.

Per-direction gates use the same coherent control snapshot as DSP. Prepared
binding indices avoid scanning every graph binding per virtual device callback.
Retained converter owners receive refreshed indices after topology changes.
Native callback quantum is preserved when split into smaller DSP blocks.
OFF clears stale source history and invalidates published capture PCM before
subsequent ON can resume; other active references/directions retain their owner.

HAL-only output backpressure now checks admission after observing the control
snapshot and OFF cleanup. Retry skips PCM, DSP and core CycleTracker advancement;
it does not freeze the OS clock or the worker's already-consumed native credit.
No second control-sampling API or duplicate render path was added.

Fresh parent-run validation: audio engine 58, audio IO 61 and scoped macOS 99
tests passed, including operation counts, ON/OFF/ON owner retention, stale PCM
discard, split native quantum, independent directions, and OFF acknowledgement
under failed capture admission. Service library 258 and desktop binary 48 tests
also passed. Strict all-target Clippy passed for engine, IO, service, desktop,
protocol and control server; workspace Rustfmt and updated Rust API docs passed.
The engine binary, production UI and normal debug app bundle rebuilt successfully.
The Vite large-chunk advisory is unchanged. Windows/Linux callers were updated
for the typed optional report, but native builds/execution on those OSes were not
run on this macOS host.

## Stream Deck interaction follow-up

- Replaced the generic action chooser with Power, Mute, Volume and Recording.
  Separate icons and Korean/English action names/inspector labels are packaged.
  Existing Power action targets and custom user titles remain intact.
- Volume keys select up/down and 0.5/1/2/3 dB steps. Dials use signed ticks and
  press-to-mute. Discovery lists only nodes with an actual gain control.
- Shared `ControlSession` and composed `VolumeGesture` use the existing node API.
  Bounded pending input retains the desired absolute gain, clamps each event,
  and drains only after the exact self-authored revision is active. Expired input,
  delayed acknowledgement/readback, OFF, deletion, target/session/topology changes
  and rejected/uncertain writes discard remaining input; failed writes never replay.
- Parent rebuilt and ran 35 plugin tests successfully, including the actual
  bundled SDK with all four action IDs, encoder feedback and Korean labels.
  Official CLI validation and packaging passed. Version 0.1.0.2 installed via
  the normal Stream Deck confirmation; built/installed plugin.js SHA-256 matches
  `0118481a452c7ddaa7e4b4b16b7b5e6ab55b2c8cd6d754eaff92124b7f88fbb5`.
- Actual Stream Deck UI displayed all four Korean actions. Selecting the existing
  microphone Power key showed the retained graph, microphone and custom title.
  No profile settings were rewritten to arrange a test layout.
- Before app restart, a fresh snapshot confirmed recorder stopped and both audio
  nodes ON (the user's latest state, not an earlier fixture). Normal Cmd-Q stopped
  desktop 53941 and engine 53964 and released port 19341. The new bundle launched
  as desktop 61263 / engine 61269 with those states and microphone gain 0 preserved.
- Against the new running app, eight volume gesture events through the same local
  IPC produced two writes, confirming 0 -> -2 -> 0 dB. Structure stayed 479;
  revisions 479 -> 481. During that window: native connection starts/retirements
  0/0, renderer starts 0, mutation errors 0. Original gain was restored by the
  acknowledged inverse gesture, not an unconditional stale snapshot write.
- This proves installed IPC/gain integration and SDK simulation, not physical
  encoder feel. Mobile ON/OFF was user-confirmed before this update; Mobile volume
  interaction and actual rotary hardware remain user/device validation boundaries.
