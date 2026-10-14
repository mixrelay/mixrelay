# Recording-device content latency

HAL input latency is the common validated upstream content delay for the virtual
recording device, in that device's nominal-rate frames. Device output latency,
stream latency and safety offset remain zero. Ordinary playback directly into
one virtual device adds no HAL sample-time delay. Zero also represents unknown;
CoreAudio's integer property has no separate unknown representation.

## Current implementation status

The runtime publication and HAL property/gating path are implemented. Native
IOProc input/output sample time, host ticks, rate scalar and validity flags reach
the direct renderer as a borrowed stack value without allocation or locks. The
host timebase is prepared before rendering. The renderer uses native output
sample/host coordinates when valid, retaining its existing fallback for missing
native timestamps.

`RenderPlan::destination_arrivals` retains terminal arrivals, including hosted
transport/intrinsic delay and route compensation. It accepts causal source
arrival observations in source-binding order. Missing observations propagate
unknown. A measured arrival that would require different compensation histories
is rejected as unknown rather than claiming histories that audio never applied.
This is a planned control-side calculation, not proof that a pending history bank
has activated.

`ContentLatency` sums isolated measured bridge/queue residence and destination
scheduling residence with planned processing/compensation and SRC algorithmic
delay. It converts once, rounding upward, to destination-rate frames. Timestamp
intervals must identify the same content in a common host clock; device sample
counters from independent devices must never be subtracted. Processing and SRC
must not be counted again inside residence measurements. `LatencyHysteresis`
retains a known value until three consecutive observations move beyond a supplied
threshold in the same direction; unknown invalidates immediately. Reset it on
rate, clock epoch, graph or lease changes.

**Measured residence is not yet wired through the PCM FIFOs, SRC, application
capture or HAL destination scheduling.** The engine reporter therefore publishes
`None` for every current virtual sink, and installed recording apps continue to
see zero. The arithmetic and hysteresis are tested primitives, not a completed
end-to-end latency estimator. No installed-driver latency agreement is claimed.

## Ephemeral authority and compatibility

The independent Driver API adds `/api/v1/driver/latency` with claim, publish and
release messages. The service issues a generation bound to engine incarnation
and process ID. A different active engine cannot claim that authority. Publishing
or releasing an old lease is ignored. Destination publications contain endpoint
ID, actual nominal rate, lease, applied graph revision and optional input frames.
Older destination revisions are ignored. Three seconds without a successful
publication expires the lease on the service's maintenance pass. Normal engine
shutdown explicitly releases it. This state never advances or persists the
inventory revision.

The existing v1 XPC inventory dictionary adds optional `input_latencies` metadata.
The Rust ABI of the inventory snapshot is unchanged. Old HALs ignore the field;
new HALs interpret absent metadata from old services as unknown. The engine treats
an old service's HTTP 404 as compatible absence. Metadata is typed and bounded,
rejects duplicate identities, and is validated before authoritative inventory
changes. HAL stale lease/revision fencing is scoped to the authenticated XPC
connection; reconnect resets the old authority. Disconnect, device configuration
changes and loss of the OS engine lease clear effective latency.

## Engine-only gate and notifications

Each device has 256 preallocated atomic client slots. Registration records process
identity on control. Start/Stop owns activity; the C bridge marks a client only
when a WriteMix render operation is observed. Capture-only registration does not
invalidate the value. A rendering client whose PID differs from the published
engine PID, including an unresolved identity, makes the effective value zero.
More clients than can be tracked also fail closed until the excess clients leave.
No client/identity/control mutex is taken by the IO marker or latency/safety
property reads. Stop/Remove and device-rate changes cannot expose obsolete frames.

The control tick coalesces IO-side gate transitions and calls PropertiesChanged
for device `ltnc` on input scope. Publication also sends this notification only
when the effective integer changes. Host callbacks run after registry, inventory,
device and identity locks are released. Tests reenter the property API while
holding control locks and assert notification callbacks own none of those locks.

## Remaining measurement integration

Carry per-content host-time provenance through native and graph FIFOs and SRC,
including partial reads, converter phase, algorithmic delay, discard, underflow,
overflow, restart and clock epochs. Each source must include validated acquisition
latency; application capture and virtual-source boundaries are not zero roots.
Measure destination scheduling against the actual consumed/scheduled content,
not the most recent callback timestamp or nominal queue target.

Publish measured source arrivals to the prepared compensation owner and acknowledge
exact active observations before reporting. Every contributing path must arrive
at one effective time under the actual installed histories; otherwise report
unknown. Aggregate multiple graph bindings to one physical endpoint before
publication (the runtime contract rejects duplicate endpoint IDs). Connect the
measured destination result and per-endpoint hysteresis to the reporter, resetting
on revision/rate/clock/lease transitions. These changes remain undone.

## Installed verification

Release/install work was not performed by this change. Before a release, run
`pnpm signing:check` using the repository's restored signing configuration, then
`pnpm release:macos --output EMPTY_DIRECTORY`. Install the resulting signed PKG
through the normal installer and restart the product/CoreAudio as its installer
requires. Never leave a signing authentication window pending.

1. Run `pnpm probe:macos-roundtrip --list` and copy exact canonical UIDs A and D.
2. With the engine stopped, run `pnpm probe:macos-roundtrip --latency D_UID`.
   Both device scopes, stream latency and safety offset should be zero.
3. Run `pnpm probe:macos-roundtrip --seconds 5 D_UID` as a direct HAL baseline.
   It measures ordinary playback/capture, not an engine-only destination.
4. Start an A-to-D engine route, with no ordinary playback client writing D.
   Run `pnpm probe:macos-roundtrip --latency D_UID`; the read-only mode creates no
   IOProc and does not disturb the gate. **Current implementation still reports
   zero here because measurement integration is unfinished.**
5. Once measurement integration is complete, run
   `pnpm probe:macos-roundtrip --json --seconds 10 A_UID D_UID` with a transparent
   signal path, first dry and then through a plug-in with reported intrinsic delay.
   Compare D input frames with measured host-time delay converted at D's rate,
   allowing the stated hysteresis threshold and observation variation. Confirm the
   recording IOProc is not also writing D; a render operation correctly gates to zero.
6. Add ordinary playback on D: its reported input latency must become zero. Stop
   that client: the validated engine value should return after the control tick.
   Change rates/graphs, inject transport loss and stop/kill the engine: obsolete
   values must clear. Repeat with old/new service/HAL combinations; missing fields
   must remain safe zero. Nonzero agreement cannot pass until step (a) is finished.
