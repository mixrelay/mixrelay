# macOS native policy extraction

Scope: macOS-first cleanup before porting. Four independent implementation slices
were integrated by one verifier, with read-only ownership review during and after
changes. No live app/engine replacement, device creation, driver install, graph
mutation, VM, commit or push was performed.

## Responsibility changes

- Device callbacks: `device_buffers` validates all native stream metadata and
  projects the selected channels to/from interleaved scratch. `audio_buffer`
  supplies the shared 64-bit CoreAudio trailing-array ABI, using raw field addresses
  rather than a reference whose extent covers only the sized header. It shares
  metadata representation with the tap, not their different layout policies.
- Aggregate groups: `aggregate_group` owns 400 bytes of immutable member geometry,
  freshness, UID order, active membership, drift readback and channel binding.
  Existing engine `group_plan` retains admission and primary selection. Partial
  create and failed destroy retain the native/CF owner for cleanup retry.
- Application tap: `process_tap_input` owns Float32 format/geometry validation,
  input layout selection, ring delivery and independently drained atomic peaks.
  Queue rejection still contributes valid input frames to clock/meter observations.
  Session admission, refresh/epoch and unregister/retry lifetimes remain native.
- Application source transport: Rust owns request/reply handling, FD/mapping,
  producer lease/epoch, snapshot/reset/write and destruction. Registry and attached
  producer share `application_source_mapping`; the previous registry-only mapping
  file is removed. The remaining 20-line C file handles only XPC's blocks-based
  connection. The transport crate's build owns this adapter; desktop does not
  compile a second production copy. Manual native fixtures link the same adapter.

No old implementation is retained as a fallback. Test and benchmark C callers
remain to exercise the actual C ABI. New runtime dependencies: none; the existing
`cc` build dependency is added to the transport crate to compile the small adapter.
The adapter explicitly targets macOS 14.2, fixing a deployment-target warning
found during integration.

## Ownership and failure evidence

26 new Rust tests: device projection 5, aggregate 8, tap 5, source transport 8.
They cover all 1..64 channel widths, bit-exact NaN payloads, packed/planar and
mixed streams, a selection crossing stream boundary 64, aggregate offset 100,
disabled unselected streams, malformed geometry and unchanged rejection sentinels.
Tap checks include finite-only peaks, queue-full observations, bounded meter
output and 1,000 callback/drain iterations with zero allocator traffic.

Source checks cover descriptor adoption, short backing storage, header/geometry
rejection, stale leases, producer contention, exhausted epochs, allocation failure
and callback allocation counts. In a dedicated child, `mprotect` verifies the
mapping's first page is unmapped after destruction and `fcntl` confirms FD closure.
This is not an OS observation of every page or a real system OOM experiment.
Source inspection verifies the original mapping extent is passed to `munmap` and
that failed owner allocation drops the local mapping and descriptor. Stale producer
destruction cannot stop a replacement lease. Local pump operations remain exclusive;
remote users access the shared atomic protocol, not Rust owner references.

## Matched callback measurements

Same arm64 macOS host, Apple Clang 21 `-O3`, Rust 1.96 release defaults, no
target-CPU flags or `RUSTFLAGS`, no concurrent build/test/benchmark work. Before
executables were built before deleting their production C policies; no old source
was retained. Each unchanged harness uses 20,000 iterations, one warm-up and seven
timed passes. Tables show the final sequential before/after comparison's median
nanoseconds per callback plus the opposite queue operation, not end-to-end latency.
Creation/destruction is excluded. Both sides retain the same previously converted
Rust PCM/capture queues and validate resulting PCM. No actual audio devices open.

Device benchmark: `MRDeviceBuffersBench.c`, capacity 16,384; input means callback
capture plus worker read, output means worker write plus callback playback. C17,
blocks, strict warnings; both harness builds use identical flags and link CoreAudio
and CoreFoundation. The XPC adapter's deployment target is not a timed path.

| Channels / frames / layout | C output | Rust output | C input | Rust input |
| --- | ---: | ---: | ---: | ---: |
| 2 / 32 / packed | 44.950 | 37.650 | 46.650 | 32.650 |
| 2 / 32 / planar | 50.250 | 70.700 | 46.700 | 57.100 |
| 2 / 256 / packed | 301.250 | 133.950 | 293.550 | 109.400 |
| 2 / 256 / planar | 323.000 | 287.000 | 286.100 | 259.350 |
| 8 / 32 / packed | 160.800 | 71.200 | 150.100 | 73.650 |
| 8 / 32 / planar | 182.550 | 209.600 | 172.100 | 194.700 |
| 8 / 256 / packed | 1164.300 | 624.050 | 1177.800 | 530.700 |
| 8 / 256 / planar | 1224.850 | 1187.450 | 1185.550 | 1202.550 |
| 64 / 32 / packed | 1142.450 | 605.200 | 1142.350 | 511.650 |
| 64 / 32 / planar | 1354.550 | 1544.250 | 1302.800 | 1523.450 |
| 64 / 256 / packed | 11497.050 | 5759.350 | 9009.700 | 8461.050 |
| 64 / 256 / planar | 10762.650 | 10157.600 | 9482.550 | 8732.600 |

The first implementation called a dynamic slice copy for each planar sample and
regressed about 5–7 times. Production mono-plane handling now uses scalar iterators;
a complete packed stream uses one full-slice copy. Mixed/subset copies keep their
validated ranges. The final 256-frame packed stereo medians fall about 56%/63%
(output/input). Short planar blocks remain 13–41% slower in this comparison;
their absolute added time ranges from about 10 to 221 ns. This is not a universal
no-regression claim. The additional metadata validation is retained before transfer.

Tap benchmark: `MRProcessTapInputBench.m`, same capacity/iteration schedule;
Objective-C ARC, blocks, minimum macOS 14.2 and strict warnings on both sides.
Uses constant finite PCM, null timestamps and queue drain; it does not exercise
concurrent meter clearing or frequent peak replacement.

| Channels / frames / layout | C | Rust |
| --- | ---: | ---: |
| 2 / 32 / packed | 84.100 | 88.050 |
| 2 / 32 / planar | 81.300 | 84.150 |
| 2 / 256 / packed | 570.450 | 568.600 |
| 2 / 256 / planar | 540.850 | 542.950 |
| 8 / 32 / packed | 224.550 | 220.650 |
| 8 / 32 / planar | 251.050 | 251.050 |
| 8 / 256 / packed | 1667.050 | 1776.100 |
| 8 / 256 / planar | 1928.200 | 1972.050 |
| 64 / 32 / packed | 1961.350 | 1919.650 |
| 64 / 32 / planar | 2078.750 | 1928.300 |
| 64 / 256 / packed | 14090.650 | 14349.550 |
| 64 / 256 / planar | 15234.750 | 15030.900 |

Tap results include small regressions as well as gains; no overall speedup is
claimed. The preceding comparison's 64-channel/256 packed result changed from
14347.450 to 12495.450 ns, while the final comparison above changes direction:
host variability must not be presented as a reliable improvement.

Aggregate active membership no longer re-resolves each UID in the nested search:
source-level call counts fall from n² extra CoreAudio UID queries to zero (4 for
two members, 1024 for 32); the preceding n freshness queries remain. This is an
operation-count comparison, not measured OS latency. Source-transport wall time
was not measured: there was no identical retained producer benchmark before its
replacement. Its queue/clock algorithms remain the existing shared implementation;
lease, ownership, failure and no-allocation tests are the alternative evidence.
No source-transport speedup or real XPC connection latency improvement is claimed.

Reproduction: build `mixrelay-macos-transport` and `mixrelay-pcm-ring` in release
mode, compile the two checked-in harnesses against their archives and the native
frameworks described above, then run one executable at a time. Raw local results
are ignored artifacts under `target/test-artifacts/{device-buffers,process-tap}-benchmark/`.

## Final integration gates

354 selected tests passed with zero failures or ignored tests: transport 110,
publication models 2, engine library 214, native contracts 1, device boundaries 13,
desktop clock pump 4 and PCM ring 10. The native contract selection invokes the
current C/Objective-C fixtures against Rust, including 1,000 tap teardown/recovery
cycles. C/Objective-C fixtures have ASan/UBSan; the Rust library is not sanitizer
instrumented. The existing 512-cycle native owner stress test reports zero owners
remaining and macOS `leaks` reports zero leaked heap bytes in its isolated child.

Changed-crate all-target strict Clippy, formatting, engine/desktop linking builds,
transport documentation, dependency audit, whitespace and feature-matrix checks
passed. The test-fixture desktop configuration also links successfully; afterward
the ordinary desktop configuration was rebuilt. Neither binary was launched.
Intel macOS transport/tests type-check passed; Intel execution is unverified.
Linux/Windows port work and live native device acceptance were not run.

Independent source review found no remaining blocker in these four changed
boundaries. Native stream-format admission, registration/priming, tap session
refresh/retirement, and HAL bridge policy remain open work, not completed cleanup.
