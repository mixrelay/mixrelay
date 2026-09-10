# macOS resource ownership refactor

Date: 2026-09-08. Local macOS only, one worker. No installed engine restart,
driver replacement, VM startup, user graph edit or default-device change.

Follow-up: [Aggregate planning boundary](2026-09-08-macos-aggregate-plan.md)
moves compatibility policy to Rust and extends stale-plan/partial-owner checks.
Counts and native responsibilities below describe the preceding slice.

## Boundaries implemented in Rust

- `macos_devices.rs`: graph resource reconciliation and runtime status.
- `spec.rs`, `aggregate.rs`, `retirement.rs`: deterministic selection, retained
  Aggregate owner, bounded readiness checks and control-side retirement. The
  32-group admission bound includes failed native releases; no per-failure C
  timer/thread releases ownership early. Even partially created native groups
  transfer to a Rust guard before creation status is interpreted.
- `native.rs`, `device.rs`, `worker.rs`: native allocation, queue handle and
  connection-worker lifetimes. A native owner retains its Aggregate and its
  admission permit until callback teardown is acknowledged, including detached
  startup/shutdown workers. The process bound is 256 native owners, matching
  the default graph node ceiling; repeated failed teardown cannot bypass it.
- `bridge.rs`: converter state and bounded queue policy, mutated by its worker.
- `format.rs`, `buffer_control.rs`, `buffer_transaction.rs`: format staging, client buffer preflight,
  deduplicated native-device writes, reverse rollback and worker publication.
  C now supplies buffer facts and a single setter call. Rust validates range,
  writability and readback, rejects changed device identity at connection, and
  restores owned buffer changes if subsequent callback registration fails.
  No-op snapshots are rechecked too: external edits fail application without
  being overwritten. This is optimistic readback, not an atomic OS transaction.
- `handoff.rs`: separate graph-coupled and same-spec recovery lanes. Format
  preparation reserves both lanes through commit/cancellation. Completion is
  signalled only after the audio consumer has published the retired owner.

This corrects two lifecycle defects: recovery no longer waits indefinitely for
an unrelated graph revision, and background recovery cannot replace resources
retained by a prepared format transaction. Geometry changes still activate only
with the corresponding DSP plan. No callback-side allocation or native query was
introduced by the handoff; retirement remains on the control side.

## Validation

- Engine service library: 93 tests passed, including native resource failure
  rolling session format back, preserving graph/revision and cancelling staging.
- macOS device unit boundary: 32 tests passed (six opt-in native tests excluded).
  Includes format reservation, both handoff lanes, seven buffer transaction
  failure/ownership cases, bounded concurrent admission, conversion continuity,
  buffer snapshot ABI/acknowledgement, partial creation failure and retirement.
- Control UI: TypeScript check and 183 focused tests passed, with no unhandled
  errors. Corrected the new dropdown test's invalid query option. jsdom's missing
  scrolling API is modelled in test setup; actual option selection is still tested.
- `cargo clippy -p mixrelay-engine-service --all-targets -- -D warnings` passed.
  No lint thresholds were relaxed and no warning suppression was added.
- Native ASan/UBSan callback, mixed-channel, epoch, SPSC and teardown checks passed.
  Shared PCM ring immediate-read/epoch tests passed; its default prefill policy
  for non-macOS consumers remains unchanged.
- Owned native fixture checks passed: four removal/reappearance cycles, real
  32-sample callbacks before/after reconnect, and 44.1/96/48 kHz graph phases
  against unchanged 48 kHz native IO, with zero FIFO overflows in each short phase.

One repeated native run (`/tmp/mixrelay-device-io-check.HoBclK`) failed at initial
device preparation in the SRC fixture. Its original boolean error did not locate
the failing operation. The native connection boundary now returns preflight stage
or native status for Rust reporting, without retries or accepting failure. The
subsequent full run (`/tmp/mixrelay-device-io-check.jdS2a3`) passed all three tests.
**The intermittent startup failure is not considered fixed by that rerun**;
capture its new stage/status on recurrence and keep native startup acceptance open.

After moving Aggregate lifetime and buffer policy to Rust, the full native suite
also passed in `/tmp/mixrelay-device-io-check.fs6tpu`. A new explicit test opened
and retired 20 fresh fixtures at alternating 44.1/48/96 kHz graph rates without
retrying failed graph preparation: all 20 succeeded. This increases regression
coverage, but does not establish the earlier intermittent failure's root cause.
The standalone fixed-member C probe is compiled by the default sanitizer script
to prevent FFI signature drift; hardware execution remains explicitly opt-in.

### Reproduced discovery race and scoped correction

The next full run (`/tmp/mixrelay-device-io-check.nsBcfI`) failed at startup
iteration 6, while the other three Rust live tests and native sanitizer checks
passed. A diagnostic repeat failed at iteration 15 with **device=0, frames=0**.
The creator process had resolved the same new UID and both stream layouts before
its handshake, but the engine process could not yet resolve that UID. Thus the
observed failure precedes buffer validation or any native setter/registration.
The earlier generic failure cannot retrospectively be attributed with certainty.

Rust initial preparation now has a distinct, cancellable discovery phase:
at most 50 snapshots/49 waits of 10 ms, only while no device is resolved.
A known device's bad buffer property, unsupported request, failed write or failed
callback registration is not retried. Normal format edits still fail immediately
on unavailable snapshots. Tests assert bounded absence, cancellation before query,
delayed publication, and immediate rejection of known-device failures. An already
available device adds no wait; this is startup readiness, not audio queueing.

After that correction, five consecutive executions of the same 20-startup test
passed: **100/100 fresh fixture connections**, alternating the same graph rates
and using the same native fixture binary. Before correction, repeated executions
could fail at index 6 or 15. This is observed reliability evidence, not a claimed
statistical failure-rate estimate or proof for every CoreAudio startup failure.

Final combined run (`/tmp/mixrelay-device-io-check.uQkHXm`): native ASan/UBSan
checks and **all four Rust native tests passed**, including another 20 fresh
startups, 32-sample apply/reconnect, four hotplug cycles and three SRC phases
with zero FIFO overflows. Final Clippy with warnings denied and diff whitespace
checks passed. No test failure was ignored or converted into a success result.

## Memory evidence and limits

`bash scripts/check-macos-device-memory.sh` runs the Rust test executable under
macOS `leaks`. The isolated probe performs 512 native allocation/retirement cycles
at 1, 2, 8 and 64 channels. Releasing the graph owner while the worker owner exists
does not release native queues/admission; releasing the final owner returns the
native-owner counter to zero on every cycle.

The memory script additionally covers 128 partial Aggregate creation failures
and, with two explicit compatible output UIDs, a real private Aggregate held by
two callback owners. Dropping one owner retains the group; releasing the last
returns group admission to zero. The explicit local UIDs were
`BuiltInSpeakerDevice` and `io.mixrelay.audio.endpoint.default`; no rate/default
changes were made by this lifetime probe.

Observed on the final buffer-policy/discovery build:

| Probe | Final retained owners | Reported leaked bytes | Physical footprint |
| --- | --- | --- | --- |
| 512 native create/retire cycles | 0 | 0 | 4800 KiB |
| 128 partial group creation failures | 0 group permits retained | 0 | 6288 KiB |
| Real private group, two callback owners | 0 group permits retained | 0 | 8401 KiB |

The tool also reported restricted-process/readonly inspection limitations.
This is not proof of zero leaks in every plugin, driver, OS, or long-running
failure mode. A vendor that never acknowledges unregistration must retain its
callback storage rather than be freed unsafely; admission bounds this retention.
The final native destructor also retains its group/permit while retrying instead
of relying on a release-build-disabled assertion. Such a permanently blocked
vendor can still delay teardown; it is not claimed to be successfully recovered.

Removed one stale test-only native Aggregate,
`io.mixrelay.device-recovery-probe.73924`, after confirming its owner PID was
gone, it was absent from the current graph, and it was neither default input nor
output. The guarded cleanup verified its sole member was the MixRelay endpoint.
Readback confirmed removal and unchanged default device IDs 145/150. This was a
recreatable test resource, not a user device or graph node.

## Performance evidence

No new latency policy was added during this structural refactor. The identical
48 kHz stereo, 16 writes of 256 frames, 32 pump turns/no native consumption probe
retained **256 queued output frames = 5.333 ms** before and after refactoring.
The preceding queue-policy change in this task measured 2048 → 256 queued frames
(42.667 → 5.333 ms, 87.5% less queued time) under those same conditions.
These are queue budgets, not measured end-to-end latency or CPU improvements.

## Still open — do not mark complete

The user's direction is Rust-first, with only genuinely necessary small native
modules. Aggregate readiness/retirement scheduling and client buffer validation,
write acknowledgement and rollback now live in Rust. The Aggregate C file fell
from 274 to 245 lines; `MRDeviceIO.c` is currently 328 lines. These counts are
scope evidence, not performance evidence. Native CF dictionary construction,
member/property/geometry checks, PCM/registry/control and callback code still
exist in C. This does **not** complete the Rust-first migration; keep narrowing
those boundaries without claiming the remaining modules are already minimal.

Production private grouping is limited to compatible fixed device-node UIDs;
HAL shared-memory and Process Tap paths are not automatically members of that
group. Physical-device sample-rate negotiation, full physical round-trip latency,
long-duration memory/clock qualification, and Windows/Linux runtime validation
remain open. Installed-app acceptance also remains pending: native tests were
rebuilt, but the running engine was deliberately not replaced.
