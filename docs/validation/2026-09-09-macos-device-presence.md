# macOS device absence and recovery

## Contract and changes

- Preserve authored UID, channel requests, nodes and connections when physical IO disappears.
- Missing input supplies inactive silence; missing output consumes/discards a full quantum.
- Keep healthy native callback routing; absence alone is not a global session failure or XRUN.
- Reconcile returning exact UIDs on the existing control maintenance path, without a graph edit.
- Show a neutral offline indicator without disabling editing or substituting default speakers.
- Poll visible-device presence independently of application/icon discovery. Failed enumeration
  preserves the previous observation; an unresolved app lookup cannot block device updates.

The common catalog now preserves absent physical selections. The macOS adapter owns separate
live/offline binding sets and only aggregates present members. The direct renderer handles
offline bindings without reading native buffers. Recovery retires replaced owners off DSP.
An explicitly requested process-private UID is resolved locally through the existing native
snapshot boundary; it is not added to public device enumeration.

## Root-run checks

- Engine-service catalog tests: 55 passed, 0 failed, 0 ignored.
- macOS device tests: 54 passed, 0 failed, 0 ignored (20.50 s), including actual private-device
  disappearance/reappearance, offline publication, gain edits, same-UID recovery without graph
  edits, deletion preventing resurrection, all supported buffer sizes and owner retirement.
- Direct callback tests: 8 passed, 0 failed, 0 ignored. Healthy PCM plus offline branches remains
  sample-exact with no added XRUNs.
- macOS test-support: 16 passed, 0 failed, 0 ignored, including process cleanup and native unwind.
- UI changed-boundary tests: 45 passed across 3 files; typecheck and desktop bundle build passed.
- Strict Clippy for engine-service/test-support all targets, rustfmt and diff checks passed.
- Independent source review closed the app-discovery/device-poll shared-request stall finding;
  no outstanding P1/P2 in the reviewed presence/direct/ownership boundaries.

Initial native tests failed because the fixture hardcoded a deleted virtual endpoint. The
fixture now requires an explicit test endpoint UID, retained across reconnect. Tests were
rerun against two dedicated temporary 2-in/2-out virtual devices, then those devices were
removed from the verification graph. No tests were skipped to obtain the results above.

## Local application and measurement

Applied the debug engine and rebuilt desktop bundle locally. CoreAudio/HAL were not restarted.
Existing user graphs were retained. A separate `장치 복구 검증` graph routes the built-in
microphone to the explicitly selected external-headphone UID.

Steady-state comparison: same Mac, same graphs, 48 kHz, 32-sample buffer, debug profile,
microphone to headphones plus existing microphone/recorder graph, 20-second windows:

| Window | Added underruns | Added overruns | Added discontinuities |
| --- | ---: | ---: | ---: |
| Before, 15:14:59–15:15:19 KST | 0 | 0 | 0 |
| After, 15:16:00–15:16:20 KST | 0 | 0 | 0 |

This establishes no steady-state regression, not a speedup. The new process had 16 underruns
and 2 overruns before the post-start window; startup is not claimed to be XRUN-free. Earlier
missing-device failure counters cannot serve as a matched comparison because the user had
already edited the graph and reconnected hardware. Controlled native recovery and PCM tests
provide the absence-path evidence instead.

Physical headphone unplug/replug and the live offline indicator remain unverified. After
requesting the unplug/replug, the verification graph changed independently from revision 168
to 172 and contained no nodes or edges. The agent did not restore the deleted routing.
The original graph-1 remained revision 163 with microphone/recorder connections intact;
graph-2 remained empty at revision 165. Counters changed during those edits (16/2 to 38/7),
which is outside the matched steady-state windows and is not claimed to be dropout-free.

## Follow-up: inactive presentation and transition handling

The user restored microphone/headphone routing (graph-53 revision 176) and reproduced the
problem. On the 18:03–18:06 KST live check, the disconnected output received a gray dot but
kept a bright face and live incoming meters. Reconnection at 18:06:50 added 6 underruns,
6 overruns and one discontinuity. The brief reported synchronization warning was not
captured by status sampling, so its exact occurrence is not claimed to be diagnosed.

Changes made afterward:

- An explicit presentation-only offline property dims the node face and zeros its displayed
  meters. Selection, ports, gain/options, document and connections remain editable/intact.
  Accessibility includes the status in the node name. Returning inventory restores live levels.
- A native owner whose PCM bridge is not ready is inactive, not an attempted failed transfer.
  It is not relabelled as inventory-absent. Ready-owner short transfers and unsynchronized
  active bridges still report their actual results.
- Callback topology loss requests control-side reconciliation on the next maintenance pass
  instead of waiting for the periodic two-second check. Pending format/publication ownership
  retains the request until it can run. No new worker, native callback allocation or lock added.
- Successful presence publications and preparation failures have timestamped structured logs.

Root checks: 105 UI tests (6 files), 46 macOS routing tests, 7 recovery tests; all passed with
zero failed/ignored. Strict engine all-target Clippy, rustfmt, UI typecheck, Biome and boundary
checks passed. Engine and desktop debug builds succeeded; Vite retains its bundle-size warning.
The 18:15 local engine/UI update retained graph revision 176. Its post-start baseline was
376 underruns and 0 overruns; startup is explicitly not validated as dropout-free.
Physical removal at 18:16:31 KST was confirmed in the actual desktop UI: the headphone face
dimmed, both output meters stopped, and its accessible name included `오프라인`. The microphone
meter and both cables remained visible. At 100 ms status sampling, counters stayed at 376/0,
with one clock discontinuity and an approximately 105 ms observed idle-to-running transition.
No synchronization issue was observed in those samples. This does not prove the absence of
shorter transients. Post-change physical reconnection evidence is pending below.

## Follow-up: actual-format admission and lifecycle diagnostics

Physical reconnection at approximately 18:27 KST restored the output indication/meters;
counters remained 376 underruns / 0 overruns, with discontinuities reaching 2. The user still
reported buzzing/crackling. Successful presence recovery is therefore not an audio-quality pass.

Timestamped connection/bridge/renderer lifecycle logs subsequently exposed a startup mismatch:
at 18:43:04 KST the saved processing preference was 96 kHz while native devices ran at 48 kHz.
The format monitor later rebuilt processing at 48 kHz. This is evidence of premature processing
admission, not proof that it explains every reported steady-state audio artifact.

The Rust worker now admits rendering only when the pacing device's observed native rate,
prepared bridge rate and processing rate agree. Control checkpoints and pending activation
remain ahead of the gate so the format monitor can pause, rebuild and resume the session.
Independent secondary-rate conversion and offline branches remain supported. No C code,
new unsafe boundary or dependency was introduced by this admission change.

Connection attempts, bridge activation and renderer retirement now carry owner/clock/rate
context. Atomic renderer telemetry is published from the non-real-time supervision loop:
calls, frames, concurrent-dispatch misses, execution-budget overruns and maximum budget load.
This measures the Rust renderer, not the entire native callback or audible output. Counter
snapshots are approximate observation windows rather than transactional samples.

Root-run checks for this follow-up (overlapping filters, not additive unique-test counts):

- `macos::`: 46 passed, 0 failed, 0 ignored.
- `admission`: 9 passed, 0 failed, 0 ignored, including native/prepared-rate admission tests.
- `macos_devices::direct::`: 6 passed, 0 failed, 0 ignored.
- Engine-service strict all-target Clippy, workspace rustfmt check, diff check and debug
  engine build passed. The full hardware test suite was not rerun for this follow-up.
- Independent read-only review found no blocking admission/ownership issue; live startup
  below additionally exercises the gated pause/rebuild/resume sequence.

The new engine was applied at 18:51 KST and the desktop app reopened afterward. HAL and
CoreAudio were not restarted. Live log sequence:

| Time (KST) | Observation |
| --- | --- |
| 18:51:12.284633 | Rendering gated at the saved 96 kHz preference |
| 18:51:12.551065 | Native 48 kHz / processing 48 kHz bridge activated |
| 18:51:12.559647 | Rendering admitted at 48 kHz |
| 18:51:12.561237 | Direct renderer started at 48 kHz / 32 samples |

At 18:53–18:54 KST, `/status` reported running, no session issue, zero underruns, zero
overruns and zero discontinuities. Five-second renderer windows showed approximately 7,500
calls / 240,000 frames, zero concurrent-dispatch misses and zero execution-budget overruns.

The user independently removed the headphone node and routing edges (graph-53 revision 194)
before this deployment. Those edits were preserved. Because the audible output route changed,
these counters cannot serve as a matched before/after sound-quality or performance comparison.
Admission tests and ordered live lifecycle evidence establish the corrected startup boundary;
headphone-path listening/reconnection and sustained audible quality remain unverified.
