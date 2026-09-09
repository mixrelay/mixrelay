# macOS clock and demand audit — 2026-09-07

Read-only runtime/code diagnosis. No device, default, sample rate, service, driver,
PCM stream, or graph was changed. This does **not** qualify native clock pacing.

Historical snapshot, not the current implementation status: the audit below was
captured before the input-activity and native-clock work. A timer profile reveals
missing synchronization evidence; it does not by itself prove measured clock
drift or an audible failure. In particular, an explicitly idle device must not be
reported as an active synchronization failure. See the subsequent runtime/status
tests and platform qualification records for newer behavior.

## Observed state

- GET `/api/v1/graph`: revision 2, only the default duplex virtual endpoint, two
  explicit channel-preserving connections, 2 render / 2 capture channels.
- GET `/api/v1/status`, approximately 10:22:23–10:22:26 UTC: 48,000 Hz, DSP blocks
  128, `running`, `engine_timer`, `clock_not_synchronized`, no overruns. Underruns
  increased **553,645 → 554,774**: +1,129 over a 3.026-second request interval,
  approximately **373/s**. The nominal DSP block rate is **375/s**.
- Read-only CoreAudio properties for `io.tonenari.audio.endpoint.default`
  (AudioDeviceID 651): alive=1, running=0, runningSomewhere=0, bufferFrames=512.
  `AudioDeviceGetCurrentTime` returned `kAudioHardwareNotRunningError` (`'stop'`,
  1937010544). There was no running HAL clock to follow at this observation.

The current increasing counter is therefore not proof of 373 actual audible
dropouts per second. It counts insufficient source transfers, including this
known inactive HAL source. Historical cumulative values cannot distinguish real
active-stream deadline misses from these inactive-source invocations.

## Code cause

- `apps/engine-service/src/main.rs:545–583`: every timer tick attempts render
  input, processes the graph, writes capture at `frame + 512`, and advances frame
  by 128 even when the HAL is not running and no input was published.
- `MacTimeline::observe` resets on render epoch or a full-capacity overrun, but
  does not wait for a native clock, consumer demand, or initial render availability.
  `wait_for_block` uses independent `Instant`/sleep deadlines. A >4-block delay
  resets the deadline without reconciling the native sample timeline.
- `read_render_cycle` returns zero frames when published input is insufficient;
  `audio-engine/src/render.rs:411` counts every short input transfer as starvation.
- `ProcessCycle` is explicitly `EngineTimer`, with `monotonic_ns: None`.
  `session.rs:271–276` reports the timer profile as unsynchronized even when the
  nominal device rate matches. This derives a warning from the configured profile,
  not a measured mismatch; presenting it as an actual sync failure is misleading.

## Available clock evidence versus missing demand

The HAL already has a native sample/host-time relationship: `TNDeviceClock`
anchors to `mach_absolute_time` on the first StartIO, publishes seed-tagged zero
timestamps while running, and stops on the final StopIO. StartIO also invalidates
the shared render/capture epochs. `DoIOOperation` receives sample/host timestamps,
but currently forwards only sample positions into its PCM functions.

The engine cannot obtain that complete relationship from the current shared
transport. Transport version 1 has only two heartbeat counters. Render publication
is observable, but a heartbeat is neither a timestamp nor direction-specific
demand. Capture reads do **not** publish their requested frame range. Consequently
render-watermark-only pacing would stall app/hardware → virtual-capture workflows
with no OS render producer. Read-only `AudioDeviceGetCurrentTime` can observe an
already-running device clock, but does not itself start a clock or describe which
direction has clients. The SDK explicitly requires a running device.

Keep these three sizes distinct: current CoreAudio quantum **512**, DSP block
**128** (four DSP sub-blocks per observed native quantum), and fixed transport
latency **512**. The HAL zero-timestamp period is **16,384**, not its I/O quantum.
Do not infer that one DSP tick corresponds to one HAL callback or hard-code an
observed quantum as every device's limit.

## Smallest correct implementation slice to approve

Introduce a common native clock/activity observation contract, then implement it
for the existing virtual HAL timeline; do not merely change the warning or stop
incrementing a counter:

1. Explicit clock identity/epoch, running/idle/unavailable state, valid native
   sample/host-time pair, rate and quantum, producer publication and consumer
   requested-through positions. Publish coherent bounded snapshots without RT
   allocation/locking. Shared-memory extensions need version/capability negotiation
   and coordinated HAL/engine deployment; old layouts must not be reinterpreted.
2. A bounded pacer consumes native demand, subdivides native quanta into DSP blocks,
   and maintains a measured prefill/latency horizon. Consumer-only, producer-only,
   restart, sleep/wake, epoch change, and absent-client states are first-class.
   Graph/control activation must remain responsive while audio is idle. Real
   active-stream shortfalls remain counted; inactive source state is explicit,
   never fabricated as successfully transferred PCM.
3. Select the actual clock driving the route, not always the virtual device:

| Route demand | Clock source and current gap |
| --- | --- |
| Active virtual render or capture client | HAL sample/host clock plus both directional watermarks; capture demand is currently missing. |
| Physical device input/output without active HAL | The owning hardware IOProc clock; `TNDeviceIO.c:71` currently discards native timestamps. HAL inactivity must not suspend this graph. |
| App capture without active HAL/hardware | Tap/native capture clock; `TNProcessTap.m:393–396` discards timestamps and source transport currently supplies a sequential frame counter, not a host-time mapping. |
| No active native demand | Explicit idle while retaining graph/control responsiveness, not a falsely synchronized running clock. |

Multiple independent active clocks still require a declared master plus drift/SRC
handling; equal nominal sample rates do not make them synchronized. Keep the timer
fallback visible for unimplemented selections. Only report native clock authority
when the processor actually follows validated native observations.

Acceptance must include idle → active → idle, virtual capture-only from app and
hardware, render-only, clock epoch restart, 512→4×128 subdivision, injected late
callbacks/real underruns, and source clock changes. Then measure the same live PCM
workload before/after (fidelity, deadline misses, occupancy, latency); this audit
alone is neither a performance improvement nor a zero-underrun qualification.
