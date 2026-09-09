# Native endpoint follow-up — 2026-09-07

## macOS: installed restart exposed a real failure

The installed launchd service was still running its September 6 binary. The live
acceptance probe backed up its graph (one duplex 2/2 endpoint and two explicit
channel edges), restarted only that UID-owned ToneNari service, and submitted a
localized name with 64 render / 16 capture channels. The engine accepted the graph,
but a separate CoreAudio property reader still observed `ToneNari Virtual I/O`,
2 render / 2 capture. **This run failed**, and must not be reported as HAL restore
acceptance. The probe's `finally` restored the original graph/name/widths. No other
graph resources, system default devices or coreaudiod were changed. With no prior
preference file, the restored original endpoint is now persisted by the new engine.

The HAL event handler previously ignored `XPC_ERROR_CONNECTION_INTERRUPTED` after
its one initial transport request. A launchd respawn does not recreate the service's
subscription. It now resends on the existing connection, as specified by Apple's
[interruption contract](https://developer.apple.com/documentation/xpc/xpc_error_connection_interrupted-swift.var).
Terminal invalidation is not retried. The service now deduplicates authenticated
audio-server subscriptions, bounds retained peers at 16, and releases the matching
peer on error. Console-user endpoint requests cannot populate that registry.

Validation:

- Eight existing native C suites passed ASan/UBSan and their existing 100% line /
  function coverage gates; the gate was not relaxed for these suites.
- The actual HAL event handler (only the send boundary stubbed) resubscribes for
  1,000 interruptions, but not invalidation, termination or unrelated messages.
- 1,000 peer add/deduplicate/remove cycles pass under ASan/UBSan; registry count
  returns to zero. Under the same 1,000 duplicate-subscription workload, the prior
  append-only registry retains 1,000 slots versus 1 with deduplication and 0 after
  disconnect. This is a retained-reference count, **not RSS or a general leak proof**.
- The HAL compiles and signs for arm64 + x86_64; engine library tests: 25 passed.
- A read-only CoreAudio inspector and a repeatable installed-service restart probe
  are now checked in. The probe refuses other graph resources and backs up live
  routing; its assertions distinguish native properties from desired graph state.

The corrected installed-HAL acceptance run **passed after administrator approval**.
`reload-macos-hal-only-root.sh` validates the source/installed bundle identifiers,
signatures and the exact ToneNari-only driver host's mapped executable, preserves
the old bundle, and signals only that host. It does not change launchd, signing
policy, or default devices and does not restart coreaudiod. The first SIGTERM did
not terminate the old host (PID 44822). After verifying its mapped backup bundle,
the separately approved host restart bounded SIGTERM to three seconds, then sent
SIGKILL only to that verified process. New ToneNari HAL host PID 68024 started.
The installer now uses that bounded termination helper instead of interpreting
successful signal delivery as a successful reload.

Installed/built HAL SHA-256 (identical):
`2865fccc368b238db95d189e736ca48e6154e925457376a9bd9dbda623597e0b`.
Previous bundle retained at
`/Library/Audio/Plug-Ins/HAL/ToneNariHAL.driver.backup-20260907T070538Z-67432`.

The v2 acceptance probe restarted the installed engine four times. Independent
CoreAudio reads confirmed the localized name `ToneNari 복원 64·16`, 64 render and
16 capture channels after application, full-device restoration and recreation.
Partial/full graph-side deletion and the retained recreation template survived
restart. The permanently duplex macOS device is not physically deleted when graph
sides are removed; the inactive capture side was observed at its startup width of
2 during the partial-side restart. This run does not claim inactive physical-side
width preservation or physical device removal.

The original graph and native 2/2 configuration were restored, and the existing
three-second installed CoreAudio duplex PCM smoke then **passed**:
`compared=281600 matching=281600 nonzero=281600 invalid_callbacks=0` at gain 1.0.
This proves stereo routing recovered after the restart sequence, not multichannel
PCM, endurance, cross-device drift or a general memory-leak guarantee.
The post-smoke status still reports `clock_not_synchronized` for the timer-clock
adapter and accumulated input-starvation/underrun accounting (42,436 at the sampled
status read). Those existing status/clock gates are not cleared by a passing PCM
comparison, and this slice does not claim an underrun-free engine.

Successful evidence:
`/Volumes/Linear 1/test/tonenari/evidence/macos-endpoint-restart-20260907-v2`
(`applied.json`, `restored.json`, `readded.json`, `cleanup.json`, `duplex.log`).

Failed evidence: `/Volumes/Linear 1/test/tonenari/evidence/macos-endpoint-restart-20260907-v1`.
Its `cleanup.json` records restored native properties; `restored-graph.json` was
compared with the original graph and matches except for revision. Neither proves PCM. The
first failing probe did not yet save the failed native snapshot; failure output
was captured in the task and that evidence gap is corrected in the probe now.

## Windows: native descriptor profile, not dynamic product support

The optional profile input to `prepare-sysvad-bridge.mjs` transforms pinned native
WAVEFORMATEXTENSIBLE tables and pin limits together. It validates 1–64 channels
per direction, recalculates block alignment / byte rate, preserves container and
valid bit widths, rates and mode-default table indices, and uses no invented
speaker positions for discrete channels. The profile registers only the two
bridged endpoint pairs. Unbridged offload, loopback and keyword pin instances are
disabled; upstream pin IDs/physical connections remain stable. The default overlay
without a profile preserves the previous diagnostic behavior.

The dedicated 64-render / 16-capture source artifact compiled natively with WDK
10.0.26100 on the Windows ARM64 VM: EndpointsCommon and TabletAudioSample both
passed, `endpoint-profile-build-v1/exit-code = 0`. Nine focused generator/bridge
tests pass, including 1/2/8/16/32/64 arithmetic, raw 32-bit capture, invalid input,
changed anchors, endpoint selection and disabled keyword pins.

Source archive SHA-256:
`1f2d576fd63a63bf001e0c1b0ea4b97fdef80657e028b2f74365f6812f39661b`.
The first generation attempt expected eight capture entries; the pinned table has
nine (including raw). It failed closed; that directory is retained and the corrected
generation is `sysvad-profile-64-16-v2`. No previous evidence was overwritten.

**The installed Windows driver was not replaced.** Runtime user configuration,
coherent production INF/names/topology (including sample microphone geometry),
authenticated reconfiguration, AudioEndpointBuilder format acceptance, WASAPI
shared/exclusive acceptance and actual 64→16 / 64→64 PCM are still open. A compiled
table is not OS audio capability proof. The Rust/UI adapter therefore still reports
the installed fixed LAB policy; it does not falsely expose 64 channels. Dynamic
reconfiguration must notify and renegotiate with Windows, not edit UI limits or
registry format bytes behind active clients; see Microsoft's
[dynamic format change contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/dynamic-format-change).

No x64 driver, Linux distro or plugin acceptance was requalified in this slice.
Existing underrun/drift issues and release signing/isolation gates remain open.
