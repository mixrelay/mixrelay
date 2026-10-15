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

Stage 2 now measures and publishes device-source routes. The IOProc records
acquisition as callback `now.hostTime - input.hostTime` and presentation as
`output.hostTime - now.hostTime`, accepting only valid, forward host intervals.
This also measures HAL loopback acquisition for a MixRelay virtual source.
Direct owners use the timestamps of the callback that actually rendered their PCM.
Missing native host timestamps remain unknown; sample-counter fallback is never
acquisition/presentation latency evidence.

Each native and graph FIFO is sampled by its sole consumer immediately before
reading. The tap accumulates `(2 * fill - actual_read) * actual_read` and the
number of frames read. Dividing the window sum by twice its weight estimates mean
frame residence via Little's law. Failed reads contribute no residence; deadline
shortages invalidate that window. AsyncClockBridge's private input and output
batch FIFOs instead integrate retained backlog over elapsed native callback time
between scheduling invocations, divided by the frames consumed in that domain.
The elapsed interval advances even when a graph producer supplies no new PCM.
The adapter advances this measurement before pumping; additional push/pull calls
within the same invocation add no elapsed time. Newly converted scratch that is
immediately transferred contributes zero residence. PCM left in staging or scratch
across invocations contributes only the time it actually waited. Its
`algorithmic_delay_frames` is added
at the converter's output rate, separately from queue residence. Empty producer
polls do not count as deadline underruns and are not residence samples.
If a concurrent producer publishes after the fill sample but before the read,
the successful read can exceed that sampled fill. A residence-invalid window
is disturbed evidence: the reporter skips it, retaining its previous stable
value and breaking a candidate streak, even without a deadline shortage fault.
This applies to native/graph FIFO and both SRC residence invalid counters; the
next valid window can resume publication from the updated baseline.

Native callbacks also accumulate separate input/output window means of valid
`AudioTimeStamp.rateScalar` values. Device-domain frame duration is the nominal
frame duration multiplied by that direction's mean scalar; a window with no valid
scalar falls back to nominal. This applies to native FIFO residence and to SRC
algorithmic delay in the native device domain. SRC batch residence areas advance
using native frames at nominal Hz; the reporter applies that native direction's
measured scalar to both SRC residence domains, including capture-side graph output
and render-side graph input. For example, backlog retained for 480 native frames
at 48 kHz and scalar 1.001 waits 10.01 ms in either domain. Graph FIFO residence
and graph-domain algorithmic delay retain the graph rate without this correction.
Likewise, 12000 native FIFO frames at 48 kHz and scalar 1.001 take 250.25 ms.

The reporter evaluates:

```text
source acquisition
+ native input FIFO / source rate
+ SRC input/output batch FIFO residence + input SRC algorithmic delay
+ graph input FIFO / graph rate
+ prepared DeviceBus snapshot quantum(s) / graph rate
+ planned hosted transport, intrinsic delay and installed route compensation
+ graph output FIFO / graph rate
+ SRC input/output batch FIFO residence + output SRC algorithmic delay
+ native output FIFO / destination rate
+ destination presentation
```

Same-callback direct bindings contribute acquisition, planned processing and
presentation only. Mixed direct/bridged routes use the corresponding terms on
each side. All terms stay in nanoseconds (including fractional measured means and
fractional graph frames) until one upward conversion to destination-rate frames.
The nanosecond counterpart of `RenderPlan::destination_arrivals`,
`measured_destination_arrivals`, uses measured source roots and the installed
compensation delays. It never prepares different histories or changes routing.
Canonical device read references follow the real route from `A/native` into
`A/snapshot` (DeviceBus), then through processing to `D/native`. The measured
topology treats that snapshot as its feeding arrival plus exactly one prepared
`RenderFormat.block_frames` quantum at the graph rate. The source-phase read
precedes sink-phase publication into the same quantum-sized ring, whose cursor
advances by actual rendered frames; shorter callback partitions do not shorten
the delay. The nominal compensation topology still cuts causal bus boundaries.
Source contributor/clock tracking and history readiness follow through the bus,
so a source fault or epoch change also taints/resets its downstream destination.
Unknown feeding arrivals and unresolved feedback remain unknown.
The audio owner acknowledges exact installed hosted-latency observations;
unacknowledged or changed observations fail closed. A destination propagates any
unknown contributor and requires all compensated path arrivals to agree within
`max(32 destination frames, 1 ms)`. Multiple sink bindings to one endpoint must
also agree and produce exactly one publication.

Taps use cumulative atomic sums/counts with a single serialized writer and a
bounded coherent snapshot, rather than independently clearing sum and count from
control. Window differences tolerate counter wrap. Native callbacks perform no
measurement allocation, lock, IPC or syscall; the native callback's supplied host
time is used, and the timebase is read on control. Control owns all maps, averaging,
graph evaluation and publication. Synthetic timestamps and FIFO reads are covered
by allocation-counter tests; existing native/direct/converter realtime tests also
exercise the added taps.

The existing 1-second Reporter and Driver lease publish the applied graph revision,
including acknowledged control edits, as required by the Driver's revision fence.
Tap selection, measurement baselines and topology validation use the active render
plan revision. Gain/mute edits advance publication revision without replacing that
plan or resetting its measurement windows.
Each endpoint has `LatencyHysteresis`: changes outside its threshold need three
consecutive stable observations in the same direction, while unknown clears
immediately. Rate, topology revision, native owner/clock epoch, direct-mode generation
and lease changes reset its state. Restart, priming, history warmup, overflow,
deadline underflow or unready compensation in a contributing path taints the
window: retain the previous stable value, or report unknown before the first stable
window. Transfer faults and compensation-history readiness are tracked per binding
and destination path; faults on unrelated routes cannot taint the destination.
A contended or missing tap snapshot skips that window and retains its last coherent
baseline and stable value. Clock metadata is read independently of tap coherence,
so only real owner/rate/epoch/generation changes reset the context. Such windows
also break a hysteresis candidate streak. A new graph/clock
context cannot retain the old context's value.

## Ephemeral authority and compatibility

The independent Driver API adds `/api/v1/driver/latency` with claim, publish and
release messages. The service issues a generation bound to engine incarnation
and process ID. A different active engine cannot claim that authority. Publishing
or releasing an old lease is ignored. Destination publications contain endpoint
ID, actual nominal rate, lease, applied graph revision and optional input frames.
Older destination revisions are ignored. Each publication is the engine's
complete set: an endpoint it no longer lists, for example after its route is
removed, is published to the HAL as unknown and kept as an unknown entry, so its
revision fence remains and a failed clear is retried. Three seconds without a successful
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

## Remaining limits

Application capture has no measurable native acquisition timestamp in this path
and remains unknown. Device directions with missing/backward host timestamps,
unavailable rates or no complete measurement window also remain unknown. Source
latency differences outside the threshold are rejected; Stage 2 does not change
source compensation or audio routing to align them.

This is a window-mean content latency estimate, not per-sample timestamp provenance
carried with PCM. Short-term queue variation, asynchronous clocks and SRC phase can
make an individual impulse differ from the one-second estimate. Active histories
are checked against exact cached observations, but the estimator does not expose
per-sample hosted transport timing. Installed engine/HAL property agreement still
requires the signed installation and impulse procedure below. No installed audio
qualification or release/install was performed during Stage 2.

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
   IOProc and does not disturb the gate. After a stable measurement window it
   should report the validated content delay; unknown paths remain zero.
5. Run
   `pnpm probe:macos-roundtrip --json --seconds 10 A_UID D_UID` with a transparent
   signal path, first dry and then through a plug-in with reported intrinsic delay.
   Compare D input frames with measured host-time delay converted at D's rate,
   allowing the stated hysteresis threshold and observation variation. With separate
   devices the probe turns off the recorder's output streams, so it never writes D;
   a render operation would correctly gate the report to zero.

Steps 4–5 and the clearing check of step 6 are automated for one route by
`node scripts/lab/macos-latency-acceptance.mjs SOURCE_UID ENDPOINT_ID EVIDENCE_DIR
--verify-installed-latency [--roundtrip]`. SOURCE is a loopback device such as
Loopback Audio and ENDPOINT an endpoint no saved graph uses. It adds the route to
the selected graph, waits for a stable report, optionally runs the round trip,
restores the exact graph and requires the report to clear to zero. Every write
names the selected graph and runs as an engine operation, so another client's edit
or selection fails the run instead of being overwritten. `--roundtrip` needs
microphone permission for the terminal.
6. Add ordinary playback on D: its reported input latency must become zero. Stop
   that client: the validated engine value should return after the control tick.
   Change rates/graphs, inject transport loss and stop/kill the engine: obsolete
   values must clear. Repeat with old/new service/HAL combinations; missing fields
   must remain safe zero. Repeat restart/priming/underrun windows: no new unstable value may be reported.
