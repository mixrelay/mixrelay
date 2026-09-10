# Complete native owner conversion

Scope: file-level macOS cleanup before porting. Windows/Linux behavior, installed
applications, live audio devices, user graphs and the Git index are not changed.
This report supersedes the ownership-location description in the earlier
native lifecycle extraction report; its historical test counts are not evidence
for this batch.

## Ownership and dependency boundaries

- Aggregate device: the Rust transport owns CF references, device creation,
  readiness, member projections and retryable destruction. The replaced
  `TNAggregateGroup.c` is removed, together with internal geometry C exports.
- Physical device I/O: the Rust device owner separates control, callback and
  graph-consumer access. A failed native unregister retains the whole owner;
  callbacks must never borrow the entire owner or PCM ring mutably.
- Application capture: the dedicated `tonenari-process-tap` crate owns sessions,
  native resources, format refresh and retirement. Its dependency points toward
  the shared transport, so AppKit does not become an engine/HAL dependency.
- HAL control: the existing Rust HAL package owns registry/connection/timer
  lifetime and reconnect. SDK COM and required Blocks syntax remain native adapters.
- Engine dispatch: the Rust endpoint service owns listener/timer startup and
  retirement. An inactive listener failure installs an inert handler, cancels,
  activates, then releases it; directly releasing it would violate the XPC ABI.
- Device discovery: the desktop now calls the existing audio-io device inventory
  instead of maintaining a second Objective-C enumeration implementation.

Tests tied to removed private C implementations are rewritten against the Rust
owners, not skipped or retained as a second implementation. Build scripts and
native fixtures must use the same replacement ABI as production.

## Audit findings

Independent review ran during implementation and again after fixes. Closed
findings include partial IOProc outputs on native failure, CF references returned
with an error or malformed extent, cleanup-queue allocation failure, and inactive
XPC listener release. Regressions exercise handle retention, failed property
outputs, queue admission, and actual anonymous XPC finalization (32 instances).

Aggregate creation has an injected error-plus-device-ID regression. This tests
the owner transition and retry behavior; it does not execute an actual failing
CoreAudio creation call on an installed device.

## Validation status

The final changed-boundary selection passed 558 tests, zero failed or ignored.
Repeated filtered runs and spawned child-process checks are not double-counted.

| Selection | Passed |
| --- | ---: |
| macOS transport | 136 |
| Publication models | 2 |
| Process tap | 23 |
| HAL core | 49 |
| Shared PCM ring | 10 |
| Audio I/O, including discovery and clock bridge | 50 |
| Processor host | 51 |
| Engine library, including dispatch owner | 219 |
| Native callback/HAL C contracts | 1 |
| Engine device boundaries | 13 |
| Desktop clock pump | 4 |

The processor measurement test is gated by an environment variable during normal
unit runs; its actual timed invocation was run separately as recorded below.
The 512-cycle native owner stress check returned to zero owners and the macOS
leak tool reported zero leaked bytes. Sanitizers instrument the retained C
contract fixtures, not Rust code linked into them.

Final strict Clippy passed for transport, tap, HAL, native test support, engine,
desktop, audio I/O and processor host, including all targets. The release-only
tap benchmark also passed release Clippy. Desktop and engine binaries built;
Rust documentation, workspace formatting and diff whitespace checks passed.
Dependency analysis found no unused dependencies. The standalone tap discovery
fixture linked against the new Rust static library, rejected the null visitor
and enumerated 11 applications; it did not start an audio capture session.
The feature evidence index was regenerated after removing stale C paths:
35 feature groups still have 31 open release gates, not a release-ready result.

The uninstalled HAL bundle built for arm64 and x86_64, passed its single factory
export checks, ad-hoc signing/verification and a local factory/COM probe without
device initialization. Intel execution, installed HAL behavior, physical hotplug
and live playback were not tested. No app installation, launch, graph mutation,
commit or push was performed.

## Remaining native compiler boundary

Production implementation directories now retain five C/Objective-C files totaling
356 lines: tap Cocoa/AppKit construction (139), source XPC adapter (20), engine
XPC Blocks (20), HAL COM projection (166), and HAL XPC Block (11). These are not
Windows/Linux totals and exclude C ABI test fixtures. Device discovery, aggregate
device and DeviceIO implementation files are removed, not wrapped or archived.

## Whole-project unsafe audit

The audit scope is all owned Rust under apps, crates and native, across every OS,
including tests and examples; it is not limited to the converted macOS files.
A lexical snapshot across 332 files found 1,510 unsafe blocks, 28 unsafe impls,
197 public/restricted-public unsafe functions and 48 unsafe extern blocks. Regex
counts include test callbacks and comments; they are neither AST counts nor
confirmed defects, and are not a claim of a net project-wide reduction.

The concrete plugin channel-reference conversion removes both unsafe blocks in
`rack_formats.rs`. Current plugin layouts are checked before activation, including
checked bus-channel sums and the 1..64 bound. The pre-existing outer catalog
comparison already rejected a normal 64-or-fewer-channel catalog whose reloaded
layout changed to 65 channels; the audit does not claim that scenario reproduced
an immediate user-path out-of-bounds access.

Structural follow-through is tracked explicitly:

- Direct PCM views and the shared recovery consumer are implemented in the
  [subsequent boundary cleanup](2026-09-10-unsafe-boundaries.md), with separate
  validation and before/after measurements. That batch also updates the existing
  Linux recovery callers; this report's original scope and 558-test count remain
  historical, not proof of those later changes.
- Processor shared memory: domain/region ownership must precede safe accessor
  changes. Replacing a narrow state-region raw slice with MmapMut indexing can
  temporarily borrow the whole concurrently accessed mapping; the audit withdrew
  its initial recommendation to make that mechanical replacement.

Do not remove unsafe tokens by hiding operations in macros, expanding a mapping
borrow, or adding blanket Send/Sync. The DeviceIO rate projection uses safe `as`
only after finite/integral 8,000..384,000 validation; its two cast-lint expectations
are localized and documented. No unchecked float conversion, global lint change,
or relaxed readability threshold was introduced.

## Performance method

Pre-replacement executables and raw results are retained only as ignored build
artifacts in `target/test-artifacts/complete-native-benchmark`. No old production
source copy or fallback is retained. Device and tap harnesses cover 2/8/64
channels, 32/256 frames, packed/planar PCM, capacity 16,384, 20,000 iterations,
one warmup and seven measured passes with PCM assertions. Release Rust and Clang
`-O3` are used without extra RUSTFLAGS. Comparisons run sequentially with builds
and tests idle. Results must distinguish variation and regressions from speedups.

Control-thread CoreAudio start/stop latency is not measured by opening or
interrupting user devices. Native-call ordering and failure injection provide
alternative correctness evidence, not a latency improvement claim.


## Measured results

All numbers below are per-case median nanoseconds, not end-to-end device latency.
Cases are channels/frames/planar/input for DeviceIO, channels/frames/planar for
tap, channels/frames/mode for clock bridge, and channels/frames for plugin process.
A negative percentage means less elapsed time. Device and clock use the second
matched run (`*-repeat.txt`); tap and the final ArrayVec processor use `*-final.txt`.

The device and tap comparison includes their actual callback and sequential PCM
queue consumer. The new tap harness is Rust libtest rather than Clang private-C
fixture code, with the same workload but a different harness/compiler boundary;
small differences cannot be attributed solely to the ownership change. Several
cases are slower; this batch makes no universal speedup or no-regression claim.

Clock bridge measures the public push/pull path, 256 warmup iterations, 2,000
measured iterations and seven passes in each of 18 cases. Duplicate Option/FIFO
queries and repeated invariant output-capacity division were removed without
unsafe. The first run's stereo/32 bypass was 206.81 -> 84.23 ns, but the repeat
was 86.25 -> 86.81 ns, so the apparent large speedup was not reproducible.
64-channel SRC cases improve modestly in both runs; other cases vary or regress.

Plugin measurement invokes the real worker process method with a deterministic
copy plugin, 20,000 warmup iterations and seven 20,000-iteration passes. It does
not load a real third-party plugin. The initial safe full-array replacement cost
10.952 -> 43.454 ns for stereo/32. The final implementation uses ArrayVec 0.7.8
with default features disabled, preparing only present channel references. The
output reference container alone is ManuallyDrop because the host API unifies
inner/outer mutable lifetimes; it owns no heap or handle to leak. Production has
zero local unsafe in rack_formats.rs, and allocation tests pass. Compared with
the unsafe baseline it still adds cost in this measurement; safety is not hidden
behind a claim of equal speed. ArrayVec's inline storage contract was checked in
[the crate documentation](https://docs.rs/arrayvec/0.7.8/arrayvec/struct.ArrayVec.html).

### device

| Case | Before ns | After ns | Change |
| --- | ---: | ---: | ---: |
| 2/32/0/0 | 34.900 | 32.450 | -7.0% |
| 2/32/0/1 | 30.150 | 29.550 | -2.0% |
| 2/32/1/0 | 56.200 | 56.400 | 0.4% |
| 2/32/1/1 | 51.050 | 51.950 | 1.8% |
| 2/256/0/0 | 119.400 | 119.350 | -0.0% |
| 2/256/0/1 | 97.300 | 100.000 | 2.8% |
| 2/256/1/0 | 300.850 | 281.150 | -6.5% |
| 2/256/1/1 | 259.450 | 258.800 | -0.3% |
| 8/32/0/0 | 72.900 | 69.900 | -4.1% |
| 8/32/0/1 | 68.550 | 69.000 | 0.7% |
| 8/32/1/0 | 187.200 | 193.950 | 3.6% |
| 8/32/1/1 | 178.550 | 179.650 | 0.6% |
| 8/256/0/0 | 558.950 | 585.200 | 4.7% |
| 8/256/0/1 | 488.450 | 493.000 | 0.9% |
| 8/256/1/0 | 1109.100 | 1071.900 | -3.4% |
| 8/256/1/1 | 1051.600 | 1096.900 | 4.3% |
| 64/32/0/0 | 550.800 | 620.950 | 12.7% |
| 64/32/0/1 | 479.850 | 487.000 | 1.5% |
| 64/32/1/0 | 1383.850 | 1415.700 | 2.3% |
| 64/32/1/1 | 1310.200 | 1401.100 | 6.9% |
| 64/256/0/0 | 5792.850 | 5486.100 | -5.3% |
| 64/256/0/1 | 8307.650 | 8232.150 | -0.9% |
| 64/256/1/0 | 10047.350 | 10114.200 | 0.7% |
| 64/256/1/1 | 8804.500 | 8893.750 | 1.0% |

### tap

| Case | Before ns | After ns | Change |
| --- | ---: | ---: | ---: |
| 2/32/0 | 80.600 | 95.681 | 18.7% |
| 2/32/1 | 76.750 | 94.890 | 23.6% |
| 2/256/0 | 530.300 | 551.696 | 4.0% |
| 2/256/1 | 522.600 | 522.604 | 0.0% |
| 8/32/0 | 217.100 | 215.569 | -0.7% |
| 8/32/1 | 234.900 | 231.440 | -1.5% |
| 8/256/0 | 1640.700 | 1659.579 | 1.2% |
| 8/256/1 | 1838.500 | 1823.513 | -0.8% |
| 64/32/0 | 1604.500 | 1590.912 | -0.8% |
| 64/32/1 | 1820.800 | 1790.131 | -1.7% |
| 64/256/0 | 13291.800 | 12620.573 | -5.0% |
| 64/256/1 | 15087.850 | 15078.337 | -0.1% |

### clock

| Case | Before ns | After ns | Change |
| --- | ---: | ---: | ---: |
| 2/32/Bypass | 86.250 | 86.810 | 0.6% |
| 2/32/Resample | 4432.190 | 4371.790 | -1.4% |
| 2/32/Independent | 4691.020 | 4670.460 | -0.4% |
| 2/256/Bypass | 309.080 | 307.900 | -0.4% |
| 2/256/Resample | 34844.980 | 34262.980 | -1.7% |
| 2/256/Independent | 36839.540 | 36547.900 | -0.8% |
| 8/32/Bypass | 325.230 | 334.170 | 2.7% |
| 8/32/Resample | 8380.500 | 8606.420 | 2.7% |
| 8/32/Independent | 8654.710 | 8674.170 | 0.2% |
| 8/256/Bypass | 1140.620 | 1217.330 | 6.7% |
| 8/256/Resample | 66255.750 | 65850.960 | -0.6% |
| 8/256/Independent | 69849.250 | 70106.580 | 0.4% |
| 64/32/Bypass | 1992.730 | 2022.250 | 1.5% |
| 64/32/Resample | 62399.040 | 60293.230 | -3.4% |
| 64/32/Independent | 66152.020 | 63892.290 | -3.4% |
| 64/256/Bypass | 10769.900 | 10880.690 | 1.0% |
| 64/256/Resample | 479437.250 | 468969.850 | -2.2% |
| 64/256/Independent | 525719.870 | 507681.560 | -3.4% |

### processor

| Case | Before ns | After ns | Change |
| --- | ---: | ---: | ---: |
| 2/32 | 11.300 | 13.429 | 18.8% |
| 2/256 | 31.294 | 35.785 | 14.4% |
| 8/32 | 27.742 | 36.077 | 30.0% |
| 8/256 | 94.133 | 141.269 | 50.1% |
| 64/32 | 196.290 | 249.108 | 26.9% |
| 64/256 | 1013.629 | 1077.275 | 6.3% |
