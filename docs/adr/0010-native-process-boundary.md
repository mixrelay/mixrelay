# ADR 0010: Native process boundary and adapter-owned capabilities

- Date: 2026-09-06
- Status: Runtime contract implemented; native graph-clock migration incomplete

## Decision

The portable DSP consumes a validated `ProcessCycle`, not a bare frame count.
Each OS implementation supplies clock authority, clock-domain identity/epoch,
sample position, actual frame count, sample rate, optional monotonic timestamp
and an explicit discontinuity marker. `BlockProcessor::process_external_cycle`
is the production entry point used by macOS, Linux and Windows.

The DSP checks the cycle before native I/O: zero/oversized quantums, incompatible
rates, implicit clock changes, discontinuous sample positions and backwards
timestamps are rejected. Callback quantum may vary within prepared capacity.
Oversized quantums must be handled by an explicit, preallocated block adapter;
they are never truncated or treated as the prepared capacity. A clock change is
not sample-rate conversion or drift correction.

`ExternalIo` receives that same cycle and returns the actual transferred frame
count plus discontinuity state. A full block of zero samples is successful audio;
missing input is counted separately. Short input tails are cleared centrally.
Reports aggregate input starvation, output overflow and explicit discontinuities.
Impossible transfer sizes are fatal adapter-contract errors, not retryable blocks.
Previously successful sink writes cannot be rolled back after a later sink fails.

`AdapterProfile` is constructed by the linked platform implementation. The common
session layer no longer guesses support or Ready state from `AudioBackendKind`,
nor does the common graph validator hard-code application capture to 48 kHz.
Profiles provide their initial format, supported formats, capture rates, actual
clock authority and known transport delay. Live reconfiguration needs both a
declared capability and an installed native setter. Current adapters may still
declare 48 kHz-only capture; that restriction now belongs to those adapters.

One processing lease owns each running session. Duplicate ownership is rejected;
dropping the lease reports `stopped`, and requires a new session for restart.
Native resources remain owned by the platform and retired off the processing
boundary. A control-plane `AudioWorker` guard requests cooperative cancellation
and joins the worker on server exit or startup failure. Unix SIGTERM and console
interrupts enter graceful shutdown rather than abandoning detached worker loops.
This does not forcibly interrupt a hung native call; such isolation and bounded
callback drain remain separate work. macOS no longer exits the whole control service when its audio worker
fails. `processingState` and `discontinuities` are additive status fields; older
serialized statuses decode with defaults. Transfer counters include startup and
idle-input starvation; they are not OS-reported hardware xrun counters or proof
of audible corruption. Windows additionally reports its device-transport counters.

## Device lifetime is not graph lifetime

`VirtualEndpointBackend::prepare_graph_attachments` replaces the misleading
`reconcile` inventory contract. It validates/stages graph attachment changes;
it does not install or delete a system driver. Installed macOS HAL lifetime and
session-owned PipeWire node lifetime remain distinct. `GraphResourceBackend`
retains responsibility for staging native resources with a graph revision.
Unknown endpoint observation is not Ready. Linux now observes actual native
stream readiness rather than manufacturing Ready from the backend name.

## Native scheduling

Windows hardware output now uses WASAPI `EVENTCALLBACK`, an auto-reset event
registered before Start, and buffer-ready waits instead of a 2 ms polling loop.
Timeout only permits checking shutdown; it never authorizes a render operation.
The stream stops before its owned event handle is closed, including error paths.
See [Microsoft's event contract](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-seteventhandle).

Windows process capture uses the same event-wait ownership discipline. When a
hardware output is attached, the graph now follows that stream's completed native
render demand. An atomic cumulative frame counter plus coalescing wakeups bridges
the native owner and the single graph worker; no graph timer manufactures frames.
The first output in stable node-ID order is the pacing authority. Its frame count
comes from successful `capacity - GetCurrentPadding()` buffer submissions in the
negotiated stream format, not opaque `IAudioClock` ticks or wall-clock estimates.
This adds a ring handoff/native-quantum delay; it is not a measured latency promise.
Stream attachment discards preparation-time demand; delayed catch-up is bounded
by transport capacity and explicitly marked discontinuous. `CycleBlocks` retains
every remainder. An invalid native capacity fails preparation instead of silently
running a transport that is too small. A failed primary output does not silently
fall back to a timer; graph edits and removal remain available.

Windows without a hardware output still uses the explicitly unsynchronized timer
for capture-only/control graphs. Removing the output resets this timer timeline;
the local control service does not require a device. macOS also schedules DSP
with an independent timer while observing HAL sample positions. These paths still
report `engine_timer` / `clock_not_synchronized`; no native timestamp is invented.

Linux now uses native `pw_stream` endpoints/taps and a persistent `pw_filter`
callback. The filter supplies its actual clock ID, position, timestamp, rate and
quantum. `CycleBlocks` validates the entire native cycle before splitting it
into prepared DSP blocks, retaining remainder frames and clock context. PCM
transport uses bounded preallocated SPSC rings; native objects and retired graph
resources are destroyed off the processing boundary. Node latency requests a
128-frame quantum, but the actual callback quantum is still authoritative.

Next implementation boundaries, not completed features:

- Linux: complete real application discovery/capture qualification and native
  format-change/clock-loss tests; nominally identical rates do not remove drift.
- macOS: synchronize engine processing with the HAL clock and handle input/output
  demand when only one virtual side is active, rather than free-running a timer.
- Windows: implement clock-domain bridging for independently clocked capture and
  secondary outputs, native capture-only pacing, live format negotiation, and the
  installable WaveRT driver. Primary render-demand pacing is not asynchronous SRC.
- All platforms: explicit buffering/SRC/drift control for asynchronous sources,
  callback cancellation/drain and format-change transactions, device loss/recovery,
  and complete endpoint lifecycle observation. Equal nominal rates do not establish
  equal clocks. The cycle validator alone cannot establish source-clock alignment.

macOS application capture is currently owned by the desktop process and pumped
through shared memory. Its capture interface participates in real audio delivery,
not only probing. Linux/Windows capture ownership is in the engine. The process
boundary may remain platform-specific for permission/isolation reasons, but it
still needs a common format/clock/lifetime contract. No probe-only rename or claim
that this ownership migration is complete is made here.

## Verification and limits

- New cycle tests exercise variable quantums, clock changes, timestamp regression,
  invalid formats and non-mutating rejection.
- DSP integration tests exercise silent/short transfers, cleared tails, propagated
  context and impossible transfer rejection before output.
- 10,000 prepared duplex cycles perform **zero heap allocations or frees**, measured
  with a thread-local counting allocator on the actual DSP entry point. This covers
  the tested portable path, not third-party plugins or native driver allocation.
- Status tests verify independent OS labels/capabilities, no false Ready, timer
  clock warnings, single-owner processing lifetime and real transfer counters.
- Local macOS focused Rust suite: 37 tests passed; Clippy with warnings denied passed.
- Windows x64 MSVC-target Clippy passed on macOS. This is not Windows execution.
- Ubuntu guest native focused tests: 43 passed; native engine build and Clippy passed.
  The test log also retains the first lint failure, corrected in the separate
  `ubuntu-runtime-contract-build-clippy-20260906.log` completion job.
- A worker ownership test verifies cancellation, join and native-resource Drop.
  A separate Ubuntu live SIGTERM check created an actual virtual endpoint, stopped
  its owned transient engine, and verified that its FIFO/directory was removed by
  engine shutdown, without test-side cleanup. Final logs:
  `ubuntu-runtime-shutdown-20260906.log` and `ubuntu-lifecycle-live-20260906.log`.
- Ubuntu live probe: all three trials reported `processingState: running`,
  `engine_timer`, `clock_not_synchronized`, and 160/159/159 input starvations
  (including startup). Active fractions were 0.992374/0.991989/0.994152; longest
  internal silent runs were 368/240/112 frames. **All three continuity trials
  failed**. This is a smoke/diagnostic result, not a controlled speed comparison.
  Evidence: `ubuntu-runtime-contract-live-20260906.log` in the external lab directory.
  Each trial's transient engine was stopped by the owned test harness.

This is primarily a contract/correctness change, not a throughput improvement claim.
The Windows event-driven change has no before/after CPU/latency measurements:
the first actual guest capture exposed an existing activation ownership crash.
Native ARM64 toolchains, 48 focused tests, Clippy, primary-output demand pacing,
actual graph attachment/removal, and 120 capture/render/teardown cycles are now
verified. Capture-only/secondary clock migration, drift control, the installable
Windows driver, and long endurance remain incomplete. See the
[native Windows record](../performance/2026-09-06-windows-render-demand.md)
for failure details and the scope of the follow-up tests. Prior Linux FIFO measurements belong to
`docs/performance/2026-09-06-linux-fifo-continuity.md`, not this refactor.
