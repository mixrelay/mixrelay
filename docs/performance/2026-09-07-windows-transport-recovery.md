# Windows transport failure containment — 2026-09-07

## Implemented

An offline or corrupt virtual mapping no longer terminates the production Windows
graph worker. The failed owner is quarantined, DSP does not run against it, and
graph edits/removal continue to activate. A fresh validated attachment restores
the normal path. This is **not** automatic PnP reconnection or all-OS certification.

- `TransportFault` latches offline, invalid-counter and invalid-quantum failures.
  Rewriting a plausible shared header cannot restore trust. Failed reads zero-fill
  and failed writes publish no PCM or counters. Normal lack of producer frames is
  waiting, not an error.
- Failure no longer falls back to timer-driven DSP. The loop still accepts pending
  graph swaps and cancellation; faulted waits are bounded to 50 ms.
- Native event waits no longer hold the PCM mutex. An independently retained event
  owns the wait lifetime; its temporary reference is released before the next active
  mapping swap. This prevents control requests from starving behind repeated waits.
- Endpoint observations come from the active mapping, not a control-plane-staged
  graph. Detachment forgets observations without claiming the installed driver was
  uninstalled. Cached faulted mappings cannot be prepared as healthy replacements.
- The protocol now distinguishes `driverState: unavailable` from `blocked` and
  `not_installed`. Current UI consumers were updated together. Workspace/card status
  presentation is shared, all locales include the state, and policy-block labels no
  longer incorrectly name macOS on other operating systems. Unknown future states
  fall back to a visible unavailable state instead of a crash/green indication.

The new enum value requires the updated UI; do not distribute the new engine with
an older UI that does not understand it. Existing state wire names remain unchanged.

## Verification

| Boundary | Result |
| --- | --- |
| Windows ARM64 build / Clippy | Passed, build v15 |
| Windows engine-service tests | 16 library + 13 binary tests passed; 2 older interactive tests ignored, not counted as passing |
| Production worker fault fixture | 12 alternating offline/corrupt mapping cycles: attachment → fault → graph edit → removal → owner release → fresh attachment; separate faulted-shutdown/join test passed |
| Mapping quarantine | Offline/counter/quantum failures stay latched; zero read/write publication after failure; fresh mapping resumes readiness |
| macOS shared engine library | 16 tests and Clippy passed, including observation invalidation |
| Portable audio-io / graph / DSP / control protocol / server | 53 tests passed; includes `unavailable` serialization round-trip |
| UI | 65 tests, TypeScript, production build and boundary check passed; shared state labels tested in en, ko-KR and pseudo |
| macOS desktop | `cargo check --locked -p mixrelay-desktop` passed; no host app/driver restart or live audio claim |
| Lab harness | 38 tests passed; explicit guest/endpoint/process ownership guards preserved |

The fault fixture uses owned memory plus real Windows event handles and the actual
worker/exchange/reclaimer code. It **does not** disable, corrupt or unplug a kernel
device. Kernel PnP failure/unload and automated device-arrival recovery still need
separate qualification. Weak-owner release checks and bounded native cycles are not
a general memory-leak guarantee.

## Real native graph removal and reattachment

`windows-rust-bridge-audio-v11` uses actual LAB WASAPI endpoints and the production
Rust engine. First attachment: three 3-second PCM/gain/meter cycles plus a live graph
revision. Remove graph, verify standalone controller can reattach, then submit the
graph to the same engine again and run another three PCM comparison cycles. Remove
again and verify mapping release again. Both attachments and releases passed.

Capture matched-frame counts:

- First attachment: **141922 / 142018 / 142018**.
- Fresh engine attachment: **141442 / 141538 / 142018**.
- Each of the six capture cycles still reports **one discontinuity**.

### Continuity regression observation, not an improvement claim

Same VM, driver, probe v11, explicit endpoints, 48 kHz, 128-frame graph block,
256-frame startup lead and channel-1 half-gain graph. Compare the first attachment
to the preceding [refactor run](2026-09-07-windows-transport-refactor.md):

| First three-cycle attachment | Before (audio v10) | After (audio v11) |
| --- | --- | --- |
| Final observed native underruns | 2 | 4 |
| Final observed overruns | 0 | 0 |
| Startup lead | 256 frames | 256 frames |
| Capture discontinuities | 1 per cycle | 1 per cycle |

The new run's 45 status samples show 3 underruns by 0.420 seconds and 4 by 0.626
seconds, with no later increase observed during that first attachment. The increase
is retained as an unresolved result; one pair of runs does not establish whether
the cause is a regression or startup scheduling variability. The second attachment
compares native PCM but does not collect another status time series; do not infer
its native underrun count from the first attachment. No latency, throughput,
zero-dropout or CPU improvement is claimed.

## Failed attempts and cleanup

- Build v13's new recovery test timed out after the first observed fault; normal
  input waiting still held the PCM mutex. After moving event waiting outside the
  mutex, all 12 cycles pass (v15 binary suite ~1.70 seconds, not a benchmark).
- Build v14 passed the tests but failed Clippy on an exact floating-point array
  comparison. The assertion now checks zero sample bits. All failure logs retained.
- Final engine SHA-256:
  `B77D7D201AE27A793F1787F294E2C8FDF06DFD89F5BB8F77C4EE3B598E560BD8`.
- Evidence:
  `/Volumes/Linear 1/test/mixrelay/evidence/windows-transport-recovery-20260907`.
- Owned interactive test task removed after completion. Final read-only inspection
  confirms no owned engine/probe/task remains. Driver, source and evidence retained;
  no reboot, driver update, signing/policy change, host audio change or Git commit.

Product packaging/configurable Windows endpoints, native hardware I/O gaps,
cross-device clock handling, distribution-specific audio and plugin/editor
qualification remain in [product gates](../implementation-status.md).
