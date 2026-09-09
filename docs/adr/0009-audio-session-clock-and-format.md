# Audio session, clock ownership, and format boundaries

## Current implementation and qualification snapshot — 2026-09-08

This section is the current source-boundary summary. **All sections after the
historical-record marker below retain their original dated evidence and restrictions;
they are not an inventory of today's missing code.** No cross-OS completion or
unconditional SRC/drift qualification is declared here.

- The shared session transaction now stages native format resources before pause,
  preserves actual opaque plug-in state, commits a reserved native handoff at the
  matching DSP publication, and separately projects proposed-format capabilities.
  External native-rate following does not write the rate back. Saved last-accepted
  formats survive restart; external save failure warns without blocking safe adoption.
  The focused common suite passed 83 tests. See
  [session reconciliation](../validation/2026-09-08-session-format-reconciliation.md)
  and [native state checkpoint](../validation/2026-09-07-native-state-checkpoint.md).
- macOS native demand pacing and application/hardware bridges have installed HAL
  recovery evidence: the acknowledged-but-unperformed attachment request now has
  a completion watchdog, and running rebind preserves the clock epoch. Actual
  duplex, session rate round trips, and owned native SRC fixtures passed. See
  [acceptance](../validation/2026-09-08-macos-clock-src-acceptance.md).
  The next migration retains native buffer timestamps and recognizes same-device
  timelines. Private-Aggregate grouping and physical rate negotiation remain open;
  see [clock ownership](0012-clock-boundary-ownership.md). Existing SRC support is
  not completion of that migration or physical/endurance qualification.
- Linux now uses native PipeWire callbacks, negotiated stream formats and staged
  live graph-rate/block changes; the prior fixed-48-kHz/FIFO-only description is
  historical. The isolated Ubuntu live application path has passed
  44.1 → 48 → 96 → 44.1 kHz transitions. Independent-clock drift qualification is
  still pending: matching downstream graph clock IDs is not evidence of upstream
  clock correction, and initial failed measurements remain failures. See
  [independent-clock investigation](../validation/2026-09-07-linux-independent-clock-drift.md).
- Windows native hardware/application format bridges and session transactions are
  under integration. **Actual SysVAD virtual-endpoint rate-policy and engine
  qualification are still pending**; an engine-only rate change or negotiated
  shared-mode client rate cannot stand in for changing the installed own virtual
  device. Existing LAB channel/name/driver qualification limits remain applicable.
- A session's `blockFrames` is the prepared DSP maximum, not a promise that the OS
  hardware callback period equals that value. Native callback quantum, client PCM
  rate, own virtual-device rate and the chosen pacing authority remain separate
  observations. One native pacing source cannot clear a synchronization warning for
  unrelated inputs, application streams or secondary outputs without bridge evidence.

### Buffer interpretation correction from live cross-process evidence

The engine client's CoreAudio `BufferFrameSize` readback was 256 while a fresh
probe of the same ToneNari UID read 512. Those are client-local I/O views, **not
evidence of a mismatched device-global buffer**. The engine's own virtual endpoint
uses direct shared memory and owns no corresponding `AudioDeviceIOProc`; changing
its otherwise-unused CoreAudio client buffer cannot configure a DAW's callback.
The attempted full-pair runtime registration was removed. macOS retains genuine
device-global nominal-rate request/confirmation/following; the shared full-format
API remains optional for backends that actually own the relevant native I/O.
See the [buffer-property counterexample and correction](../validation/2026-09-08-macos-buffer-format-sync.md).

The user-facing buffer setting means **ToneNari processing buffer / maximum DSP
slice**, not DAW/OS-wide buffer synchronization. Linux callback observations of
117/118/128/256 frames across rates likewise need not equal that maximum. Variable
native quantum and remainder preservation are verified separately from changing
the DSP setting. A fresh process is not required to report the engine client's
buffer value, and such equality is not a product completion gate.

The [feature matrix](../feature-parity-matrix.md) continues to distinguish implementation,
native fixtures and actual OS/vendor/device acceptance. Global capability flags must
not be promoted from a common converter test alone. Physical/endurance, other Linux
distributions, real vendor UI, and unqualified driver paths remain open.

## Historical implementation record (2026-09-06)

Status: partially implemented; macOS session transactions and Windows shared-mode
hardware rendering are implemented, with native runtime validation still pending.
Native clock scheduling, complete capture reconfiguration, and clock bridging remain open.

## Follow-up: persistent session preferences (2026-09-06)

macOS now restores a supported saved format before constructing the initial DSP
plan. The initial graph endpoint metadata uses that same format; it is no longer
implicitly rebuilt as 48 kHz when the saved DSP format is 44.1 kHz.

The common store uses versioned, backend-tagged JSON, bounded to 4096 bytes. Paths:

- macOS: `~/Library/Application Support/ToneNari/audio-session.json`
- Windows: `%LOCALAPPDATA%/ToneNari/audio-session.json`
- Linux: `$XDG_CONFIG_HOME/tonenari/audio-session.json`, or
  `~/.config/tonenari/audio-session.json` if the XDG path is absent/relative.
- `TONENARI_CONFIG_DIR` can override the directory and must be absolute.

The Windows/Linux path and serialization code are cross-compiled, but their fixed
format adapters do not yet offer live preferences; startup restoration is currently
wired into the macOS adapter only. This does not enable ASIO/JACK or arbitrary rates.

An explicit apply stages and syncs a private temporary file in the destination
directory before suspension. Native format confirmation precedes replacement of
the old preference. A failed commit rolls the device back, and a failed native
rollback keeps audio suspended. Normal failure paths close/unlink only the owned
temporary file. File I/O never runs on the audio worker. A process crash may leave
an orphan temporary file; startup does not automatically delete unrelated files.
The store uses [Rust's platform replacement operation](https://doc.rust-lang.org/std/fs/fn.rename.html),
not delete-then-write. Directory sync is attempted on Unix; failure after the rename
is logged rather than falsely reporting that the committed preference was rolled
back. No universal power-loss durability guarantee is claimed.

Missing preferences keep the current default without writing it automatically.
Corrupt, oversized, incompatible, or inaccessible preferences remain untouched;
the control service still starts and reports `preferences_unavailable`. A native
startup refusal reports `stored_format_unavailable`; the device mismatch/mute
interlock remains active. `preferredFormat` reports a readable stored preference
separately from the worker's active `format`, including after failed restoration.

Reapplying the unchanged format can save/recover it without preparing new plug-in
instances or advancing the graph revision. This is safe with external processors;
actual format changes still require the as-yet-unimplemented opaque state-transfer
transaction. The UI preserves graph history in the unchanged-format case and
distinguishes saved/active values in its scroll-contained settings popover.

The host library has `save_state`/`load_state`, and the worker already loads opaque
state. Export through the isolated process control channel, editor/DSP quiescence,
and all-worker rollback are not implemented; simply calling `save_state` while
native editors continue modifying a plug-in would not provide an atomic migration.

Validation for this follow-up is build/static only. No preference apply, alternate
sample-rate restart, failed-disk simulation, plug-in migration, or listening test
has been run under the requested test deferral. Settings persistence is separate
from full graph/preset persistence, which this change does not claim to provide.
The updated development engine was restarted without a preferences file: status
reported 48000/128, observed device 48000, `preferredFormat: null`, and `issue: null`.
The revision-11 graph was backed up and restored byte-for-byte. TypeScript, UI
production build, targeted Biome, Core/Feature boundaries, macOS Clippy/build, and
Windows/Linux engine cross-target Clippy passed. Native persistence round trips
and Windows/Linux execution remain unverified.

## Follow-up: live session transaction (2026-09-06)

This section supersedes the initial fixed-format restriction below for macOS only.
`PUT /api/v1/audio/session` accepts `{expectedRevision, format: {sampleRate,
blockFrames}}`. macOS now offers 44.1/48 kHz and 64/128/256/512-frame DSP blocks.
These are DSP block sizes, not a claim about Core Audio client callback sizes.
Windows and Linux adapters reject this operation until native format control is
implemented; their reported capabilities remain fixed.

The controller serializes graph edits, parameter persistence, and session changes.
It rejects stale/pending graph revisions, prepares a new DSP/plugin plan, requests
block-boundary suspension, and waits for acknowledgement. Core Audio property-set
success is followed by observed-rate confirmation. Failed negotiation requests the
old rate; failed rollback leaves audio suspended. A late external rate change is
still covered by the mismatch/mute interlock. No claim of sample-accurate external
rate-change detection is made.

Only after native negotiation succeeds is the prepared plan published and the
worker resumed. The worker uses its active plan's format for block processing and
reports that format atomically. Maximum-sized audio storage is allocated once;
changing block size does not allocate on the audio worker or reset its sample
timeline. Retired processors and revision meter bindings remain bounded.

The footer's Core/Mantine controls now invoke this transaction through HTTP/Tauri,
wait for active revision, and refresh endpoint rate metadata while preserving node
positions. The graph is inert during the UI operation. Undo history restarts at the
session boundary (explained before applying), so old endpoint-rate snapshots cannot
silently undo native device configuration.

Remaining restrictions (preferences now covered by the follow-up above):
application capture transports remain 48 kHz without SRC. Rate changes
with captures are rejected, and adding captures at 44.1 kHz is disabled/rejected.
Block changes at 48 kHz can retain capture FIFO streams. Full plugin preset/editor
state transfer across reprepare is not implemented: sessions containing external
provider nodes reject reconfiguration rather than silently reset their native presets.
Built-in processors are reconstructed from their graph configuration. Native audio
continuity still requires validation.
No rate-change audio test has been executed, per the requested testing deferral.

## Follow-up: Windows rendering and Linux resource ownership (2026-09-06)

Windows `AudioDeviceOutput` now opens the selected persistent endpoint or default
output using WASAPI shared mode. A dedicated COM-owning worker negotiates Float32
48 kHz with system conversion, uses the actual endpoint buffer capacity/padding,
and feeds `IAudioRenderClient`. A bounded 4096-frame SPSC queue connects the timer
DSP worker to the device worker. The 20 ms requested buffer is not advertised as
measured latency. Channel counts must match the device mix format: no implicit
stereo downmix is requested. Device disappearance and ring under/overrun counters
are exposed; reapplying a failed output can reopen its stream. Unchanged healthy
output streams are reused when the graph changes.

COM interfaces, endpoint buffers, and apartment teardown remain on the owning
worker. Normal removal stops and joins that worker on the reclaimer. A synchronous
driver call that stalls during startup cannot be forcibly cancelled; initialization
times out and requests shutdown, and its detached worker releases resources if the
OS call returns. This is not a guarantee against permanently blocked OS drivers.

Windows still lacks hardware input, exclusive-mode negotiation, ASIO, and a signed
virtual driver. System rate conversion on one shared render endpoint does not
establish asynchronous clock-drift correction between devices. The reported DSP
clock remains `engine_timer`, and the generic clock-bridge capability stays false.

Linux no longer skips all DSP when its optional virtual endpoint is removed.
Application sources and processors continue processing. Source-gain edits on both
Linux and Windows no longer restart capture clients: gain belongs to DSP, not the
native resource identity. Linux's native callback/JACK work remains outstanding.
If off-thread resource retirement delays a graph swap, both workers keep rendering
the active graph and retry activation at the next boundary; they do not skip audio
blocks while waiting. The busy path uses a static error without heap allocation.

Resource exchange now reuses prepared boxed envelopes on Linux and Windows.
For the same successful resource-swap boundary, the previous path explicitly
allocated one retirement box; the new path allocates zero and defers all old-owner
destruction to the reclaimer. This is a static allocation-path comparison, not a
measured speedup. Same-condition timing/allocation profiling was not performed:
these are target-specific workers, no native Windows/Linux execution host was used,
and the user deferred tests. Cross-target Clippy and manual ownership inspection
are the current alternative evidence. macOS preallocated external sample storage
increases from 65,536 to 262,144 bytes to cover 512-frame/64-channel duplex blocks;
this is an intentional bounded capacity cost, not a memory reduction claim.

## Initial observed implementation (2026-09-06, before follow-ups above)

The existing adapters process 48 kHz / 128-frame blocks on a timer. This is a
runtime limitation, not a product requirement or a list of formats supported by
Core Audio, WASAPI, or PipeWire. The macOS HAL independently advertises 44.1/48 kHz;
therefore its nominal-rate setter previously allowed a configuration the engine
could not consume correctly. Source shared memory also carried frames without a
rate/conversion contract. Linux uses FIFO pipe-tunnel modules, not native graph
process callbacks. Windows has process capture but no hardware render adapter or
usable signed ToneNari virtual driver. Hardware enumeration on the desktop is
not hardware audio I/O implementation.

## Implemented boundary

- `audio-io::session` owns format, clock-authority, synchronization constraints,
  and adapter capability DTOs. Channel count remains independent of this session;
  a 64-channel stream does not imply a 64-channel engine or stereo downmix.
- `/api/v1/status.audioSession` exposes actual DSP format, independently observed
  device rate, explicit unsupported capabilities, and known transport delay.
  Missing observations are null, not inferred from engine defaults. Delay is not
  advertised as measured round-trip latency.
- Graph validation rejects unsupported native bindings and endpoint default rates
  that differ from the running session **before** preparing hosted processors.
- Windows starts with an empty graph, not fictional virtual I/O. Missing render
  support is rejected by the controller instead of silently discarding a sink.
- macOS queries the HAL on a weak-reference monitor thread (250 ms interval).
  The audio worker only reads an atomic observation; it never calls Core Audio
  or allocates to observe formats. On a detected mismatch, virtual input is zeroed
  and virtual output is muted. Detection is bounded by polling, not sample-atomic.
  This is a safety interlock, **not** a reconfiguration implementation.
- Desktop process capture refuses a mismatched initial rate instead of pumping
  raw frames into a differently clocked engine. This is not drift correction.
- UI shows session diagnostics through Core Mantine components / Feature logic,
  with translated lock/mismatch explanations and unavailable menu actions.

## Target architecture and remaining work

One engine session owns DSP rate and block size; individual graphs must not compete
to change one shared device's rate. Endpoint names and channel maps are separate.
The control layer will choose a clock authority and request a configuration; the
backend must return the accepted format and constraints, not just acknowledge a
requested number. Preferences must distinguish requested and active values.

Reconfiguration must prepare new transports, capture sessions, plugin instances,
buffers, and coefficients off the audio thread, quiesce I/O, negotiate OS changes,
activate one generation at a boundary, then retire old resources. Failed preparation
leaves the previous session intact. OS negotiation failure needs rollback; an
unrestorable disconnected device must stay suspended with a specific error. Do
not implement a rate dropdown that changes only a graph JSON field.

Required native work:

1. Core Audio: rate-change notifications, device-driven scheduling, timestamped
   source transport, native capture/render for hardware, and HAL/engine generation
   negotiation. Keep ABI compatibility explicit when extending shared memory.
2. Windows: WASAPI hardware capture, native-clock rendering, and exclusive formats;
   ASIO through its own backend; actual signed virtual-device driver and transport.
   A transport header or cross-compilation does not establish driver support.
3. Linux: PipeWire native process/rate/quantum callbacks and JACK adapter; select by
   runtime capability, not Debian/Kali distribution name. FIFO operation must not
   be labeled audio-server-clock synchronization.
4. Clock bridges: timestamped asynchronous SRC for independently clocked devices,
   explicit latency accounting and drift telemetry. Matching nominal sample rates
   alone never proves clock synchronization.
5. Spatial audio: preserve discrete channel order and explicit mapping; add layout
   metadata (speaker roles / Ambisonic order, normalization and ordering) before
   doing any layout conversion. No implicit stereo mix at format boundaries.

## Actual integration constraints

Ableton/other DAWs via hardware or virtual I/O require negotiated device formats;
independent physical devices additionally require a clock bridge. SPAT Revolution's
Local Audio Path is a distinct plugin-mediated transport: it requires matching
sample rate and buffer size and has host processing-order and latency constraints.
Loading its AU/VST editor does not satisfy those requirements, and this change
does not claim LAP integration or native SPAT availability on Linux.

Primary references consulted:

- [SPAT Local Audio Path](https://doc.flux.audio/spat-revolution/Ecosystem_&_integration_DAW_Automation_Local_Audio_Path.html)
- [PipeWire graph scheduling](https://docs.pipewire.org/page_scheduling.html)
- [WASAPI device formats](https://learn.microsoft.com/en-us/windows/win32/coreaudio/device-formats)
- [WASAPI rendering contract](https://learn.microsoft.com/en-us/windows/win32/coreaudio/rendering-a-stream)

## Verification boundary

No performance improvement is claimed. No audio test suite or destructive driver
reinstallation is part of this increment. Type/lint checks and cross-target Rust
compilation establish source compatibility only; Windows/Linux hardware execution,
rate-change discontinuities, clock drift, round-trip latency, plugin reprepare, and
rollback need subsequent native integration validation after implementation.

Completed checks for this increment:

- UI TypeScript, targeted Biome, and Core/Feature import boundary checks passed.
- Engine/control-server/desktop macOS Clippy passed. Engine cross-target check and
  Clippy passed for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`.
- Linux-only editor/lv2 Clippy findings were corrected without changing window
  ownership or plugin discovery behavior.
- Rebuilt and restarted the macOS development engine. Read-only status reported
  engine 48000 Hz / 128 frames, observed HAL 48000 Hz, `issue: null`, and the new
  live-session capability with 44100/48000 Hz and 64/128/256/512 frames. No format
  change was issued as a verification action.
- Preserved and restored revision 11 (one duplex endpoint / two channel routes);
  exact graph JSON and active/submitted revision matched after restart. The
  Tauri development process remained running. No audio test suite was executed.
- The Vite development server serves the new session-transaction controls, and the
  production UI build passed (existing large-chunk advisory remains). No claim is
  made that a click-through or listening check was performed in the desktop UI.
