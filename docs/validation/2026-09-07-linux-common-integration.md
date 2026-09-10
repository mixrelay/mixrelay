# Ubuntu ARM64 common integration — 2026-09-07

This check covers the graph catalog, processor control/state transport, and shared
device-worker integration in the Linux engine. It is native-platform build and
unit-test evidence. It does not qualify native PCM routing, physical device
arrival/removal, vendor plug-in state/editor behavior, the desktop UI, other Linux
distributions, or Linux x86-64.

## Reproducible boundary

- Ubuntu ARM64 VM `77F2F097-29B8-4836-80F6-358AE38A2316`, kernel
  `7.0.0-31-generic`, Cargo/Rust `1.96.0`, PipeWire development library `1.0.5`.
- Commands run as existing `state303` UID 1000. Each run uses an exclusive
  `/var/tmp/mixrelay-integration-20260907-*` source directory and the existing
  `/home/state303/mixrelay/target` compilation cache.
- Existing guest source, user graphs, service configuration, audio defaults,
  devices, and other VMs were not changed. No engine service or audio fixture was
  launched. No packages were installed. Owned source archives and logs remain for
  review; they are not cleaned up automatically.
- `scripts/lab/guest-job.sh` creates an exclusive job directory. Completion is
  determined by its `exit-code` file, not UTM's immediate command status.

The checked commands are `cargo build --locked -p mixrelay-engine-service`, then
`cargo test --locked` and `cargo clippy --locked` for the seven crates listed below.
Tests select `--lib --bins`; Clippy selects `--lib --bins --tests -- -D warnings`.
The exact runner and source archives are retained with the evidence.

## Run history

| Run | Source archive time (UTC) | Result |
| --- | --- | --- |
| v1 | 2026-09-07 09:52:35 | Engine build and 116 tests passed; Clippy failed because `mutate_catalog` exceeded the 100-line limit. Exit 101 retained. |
| v2 | 2026-09-07 09:55:13 | Following catalog helper extraction, engine build, 116 tests and Clippy passed. Exit 0; completed 09:55:44 UTC. |
| v3 | 2026-09-07 09:57:28 | Final snapshot includes the new catalog HTTP route contract test. Engine build, 117 tests and Clippy passed. Exit 0; completed 09:58:01 UTC. |

The v1 failure is preserved in `v1-build-tests-clippy.log`; it is not counted as a
passing integration run. The v2 source archive SHA-256 is
`d8ac72f1a9c99a45f247f9ac9f59429dccafc06077ec6261d3b1816fb4a14229`.

## Final v3 results

| Crate | Passed tests |
| --- | ---: |
| `mixrelay-audio-engine` | 19 |
| `mixrelay-audio-graph` | 15 |
| `mixrelay-audio-io` | 22 |
| `mixrelay-control-protocol` | 2 |
| `mixrelay-control-server` | 6 |
| `mixrelay-engine-service` | 37 library + 4 Linux adapter |
| `mixrelay-processor-host` | 12 |
| **Total** | **117 passed, 0 failed, 0 ignored** |

The final run includes all nine graph catalog regressions, independent graph
documents through switching/rename/restart, native-resource preparation rollback
with a test backend, revision conflicts, the catalog HTTP route identity and two
concurrency baselines, parameter mailbox coalescing/capacity/concurrency, state
transport failures/timeouts, and Linux recovery health/cancellation contracts.
State transport tests use controlled responses; they are not Linux vendor-format
state round trips. The native-resource failure test uses a test backend rather
than a physical device.

Final archive: `source-v3.tar.gz`, SHA-256
`897e56eb317573a64b3f4c20d401df15b24775080ea85f6c41159a5bc5b5f22b`.
Full output: `v3-build-tests-clippy.log`; completion marker: `v3-exit-code`.
The engine debug build completes in 4.85 seconds; test compilation in 4.34 seconds;
Clippy completes in 0.94 seconds. These are cached validation timings only.

No performance improvement is claimed. Build times use an existing cache, so they
do not establish a before/after performance comparison.

Evidence directory:
`/Volumes/Linear 1/test/mixrelay/evidence/native-device-parity-20260907/linux-integration`.

## Follow-up: direct parameter durability

Run `parameter-durability-v1` verifies the later durable `parameterValues` catalog
write and optional `ProcessorParameterUpdateResponse.persistenceError` contract.
It uses the same seven-crate commands, non-root account, and native-build/unit-test
scope as v3 above. Earlier archives/results are preserved separately.

The source snapshot was captured at **2026-09-07 10:29:37 UTC**. Native engine debug
build, all **123 tests**, and Clippy passed; the explicit job exit code is **0**.
Completion was **2026-09-07 10:30:21 UTC**.

| Crate | Passed tests |
| --- | ---: |
| `mixrelay-audio-engine` | 19 |
| `mixrelay-audio-graph` | 15 |
| `mixrelay-audio-io` | 22 |
| `mixrelay-control-protocol` | 3 |
| `mixrelay-control-server` | 7 |
| `mixrelay-engine-service` | 41 library + 4 Linux adapter |
| `mixrelay-processor-host` | 12 |
| **Total** | **123 passed, 0 failed, 0 ignored** |

The 13 catalog tests include accepted parameter edits surviving restart without
native re-preparation, rejected edits producing no catalog writes, storage failure
preserving the accepted live edit with a warning, later persistence recovering all
parameter IDs, and corrupt catalogs retaining their original bytes. These are
controlled backend and filesystem tests, not vendor plug-in or physical-device
acceptance. Protocol/server tests also pass with the new response contract.

Archive: `source-parameter-durability-v1.tar.gz`, SHA-256
`f3f3a9f23ee19f6a5566b4d84932444d945e956385910fa1d8c680ebced8e91a`.
Full output: `parameter-durability-v1-build-tests-clippy.log`; completion marker:
`parameter-durability-v1-exit-code`, in the same evidence directory.
Cached engine build/test-compilation/Clippy timings are 6.13/5.64/1.81 seconds.
No performance improvement or additional native audio/UI proof is claimed.

After this snapshot, the root integration applied rustfmt to the worker-crash test
in `crates/processor-host/src/lib.rs`: the three-line `command.spawn().expect(...)`
chain became one line. A direct comparison against the retained archive confirmed
that this is the only subsequent difference in that file. The Linux run above
precedes that formatting-only edit; it was not rerun for whitespace alone.

## Follow-up: graph layout and virtual source gain

Run `layout-source-gain-v1` verifies the frozen common graph-layout backend and
virtual-source gain changes using the same seven-crate build/test/Clippy commands.
The source snapshot was captured at **2026-09-07 11:46:03 UTC**. The Ubuntu ARM64
engine debug build, all **136 tests**, and Clippy passed; the explicit job exit
code is **0**. Completion was **2026-09-07 11:46:54 UTC**.

| Crate | Passed tests |
| --- | ---: |
| `mixrelay-audio-engine` | 21 |
| `mixrelay-audio-graph` | 17 |
| `mixrelay-audio-io` | 22 |
| `mixrelay-control-protocol` | 4 |
| `mixrelay-control-server` | 8 |
| `mixrelay-engine-service` | 48 library + 4 Linux adapter |
| `mixrelay-processor-host` | 12 |
| **Total** | **136 passed, 0 failed, 0 ignored** |

Layout coverage includes independent layout revisions, inactive graph layout
through selection/rename/restart, invalid coordinates/IDs, old catalog migration,
corrupt layout preservation, failed disk writes with same-revision retry, and no
native-resource preparation or audio/catalog revision advance for a layout edit.
The HTTP route and optional metadata compatibility tests also pass.

Virtual-source gain coverage includes JSON/state round trips, invalid-value
rejection, persistence through disk restart, unchanged prepared native device
specifications, PCM/meter scaling exactly once without allocation in the tested
render path, and an unaffected independent application-capture path. PCM here is
controlled unit-test input; no native audio stream, device, graph, service, UI, or
PnP scenario was exercised. Desktop proxy files are archived but the desktop
crate is outside these seven-crate checks. These results qualify neither another
Linux distribution/architecture nor another operating system.

Archive: `source-layout-source-gain-v1.tar.gz`, SHA-256
`42abb3ed0253c51ca68ce1f30453d2cbb8a7278b158207ab491e05e6226d1bce`.
The digest was checked on both the host archive and its uploaded guest copy and
matched exactly. Full output: `layout-source-gain-v1-build-tests-clippy.log`;
completion marker: `layout-source-gain-v1-exit-code`, in the evidence directory
above. Cached engine build/test-compilation/Clippy timings are 7.55/9.15/3.01
seconds; these validation timings do not establish a performance improvement.
All previous run artifacts remain preserved.

## Follow-up: application metadata and native discovery diagnostics

`app-metadata-v3` ran at 2026-09-07 12:39:55–12:40:11 UTC on the same Ubuntu
ARM64 VM, using an isolated source snapshot and the existing compilation cache.
Engine build, **155 shared tests / 0 failed / 1 opt-in ignored**, and Clippy passed.
Counts: engine21 + graph17 + audio-io30 + protocol5 + server8 + service48/adapter4 +
processor-host22. The new common scanner failure/child-lifetime diagnostics are
included in this execution; actual installed vendor discovery is not.

The opt-in installed-desktop metadata read test was then explicitly run and
passed separately. `/usr/share/applications/apport-gtk.desktop` resolved
`Report a problem...` and its installed PNG to a 5222-byte data URL. This is an
actual installed-file resolution check, not an application launch, capture,
native menu/window, service or device test. No audio settings were changed.

Archive `source-app-metadata-v3.tar.gz` SHA-256:
`404efbc2740b1a5e3d2ad7d9dd033cd9ae0be1f905d3dc9ab1819bc8ab289745`, matched on
host and guest. Evidence: `app-metadata-v3-build-tests-clippy.log`,
`app-metadata-v3-exit-code` (`0`) in the evidence directory above. v1 Clippy
failures and v2 success remain preserved. See
[application metadata](2026-09-07-application-metadata.md) for implementation bounds.
