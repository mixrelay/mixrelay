# Validation snapshot — 2026-09-06

This is **not** cross-OS product certification.

## Latest native adapter work (supersedes matching older rows below)

- macOS corrected HAL installed with administrator approval: four engine restarts
  pass native localized-name/64-render/16-capture restoration and graph-side
  deletion/recreation. Original 2/2 routing restored; three-second actual stereo
  PCM smoke matches 281,600/281,600 samples with zero invalid callbacks. This is not
  multichannel PCM or idle-clock/underrun qualification. Windows 64/16 descriptor
  profile separately compiles on ARM64 but is not installed or runtime-configurable.
  See [native follow-up](../../docs/validation/2026-09-07-native-endpoint-followup.md).

- Durable endpoint settings: Windows ARM64 build-v17 passed 39 tests and Clippy
  (2 interactive tests ignored); Ubuntu native build/25 tests/Clippy and four real
  engine process lifetimes passed. UI 74 tests passed. The restart probe resolves
  node ownership through PipeWire client IDs; older direct-node-PID cleanup checks
  alone were insufficient. No PCM quality qualification is claimed.
  See [persistence/restart record](../../docs/validation/2026-09-07-endpoint-persistence.md).

- Backend-owned endpoint settings/recreation: Windows ARM64 build-v16 passed 33
  tests and Clippy (2 interactive tests ignored); Ubuntu endpoint-settings-v3 passed
  19 tests, native binary test compilation and Clippy. UI 72 tests passed. These
  checks do not add Windows variable-channel devices or qualify live audio.
  See [settings validation](../../docs/validation/2026-09-07-endpoint-settings.md).

- Windows virtual transport faults now quarantine the mapping without terminating
  the graph worker. Native worker fixtures pass 12 failure/edit/remove/reattach
  cycles and faulted shutdown. Windows build/Clippy and 29 engine tests pass
  (2 older interactive tests ignored). The real driver passes six PCM cycles
  over two fresh engine attachments and repeated release. First-attachment
  observed startup underruns increased 2→4 versus the preceding run; this remains
  unresolved, not a continuity pass. See [failure containment evidence](../../docs/performance/2026-09-07-windows-transport-recovery.md).
- Windows v2 transport wire/layout validation is now separate from native handle
  ownership and tested on macOS and Windows. All 26 Windows engine tests pass
  (2 older interactive tests still ignored), plus the actual three-cycle LAB
  graph/PCM/meter/revision/removal test. Same-condition final underruns remain 2,
  overruns 0; one capture discontinuity per cycle remains. This is a regression
  pass, not a performance improvement or configurable multi-channel device support.
  See [transport refactor evidence](../../docs/performance/2026-09-07-windows-transport-refactor.md)
  and [remaining product gates](../../docs/implementation-status.md).
- Rust now consumes the SysVAD shared mapping in explicit LAB mode: actual graph
  channel/gain routing, meters, live revision and removal/re-attach pass. ARM64
  build, 20 engine tests and Clippy pass. A 256-frame startup lead adds 5.33 ms;
  same-condition observed initial underruns changed 3→2, not zero. Configurable
  product endpoints and normal UI remain disabled. See
  [Rust bridge qualification](../../docs/performance/2026-09-07-windows-rust-bridge.md).
- The separate SysVAD/ToneNari diagnostic bridge now passes three actual shared-PCM
  render → channel 1 × 0.5 → capture cycles on Windows ARM64 (48 kHz, 2 → 1 channels).
  Each cycle matches about 140k capture frames, but reports 3–5 capture underruns and
  one discontinuity; no continuity/quality qualification. PCM conversion has 500k
  sanitizer-tested cases. This is a native test controller, **not the Rust product
  adapter**. See [bridge evidence](../../docs/performance/2026-09-07-windows-sysvad-bridge.md).
- Microsoft SysVAD upstream ARM64 baseline builds without audio-source/project
  changes, installs under a separate sample hardware ID, and creates four healthy
  render plus four capture endpoints. Three explicit WASAPI render/capture cycles
  pass with advancing clocks and nonzero sample-tone capture. Each capture reports
  one discontinuity: continuity is not qualified. Universal INF validation passes;
  stricter Windows Driver isolation validation fails. This does not repair or qualify
  the existing ToneNari driver, which still has zero MMDevice endpoints. See the
  [SysVAD reuse baseline](../../docs/performance/2026-09-07-windows-sysvad-baseline.md).
- Windows ARM64 WaveRT driver (`windows-driver-v8`, version 0.1.0.2) passed native
  build/link, WDK INF validation and 4,096 shared PCM-copy cases. VM-only test signing
  and installation are complete; the running kernel loads the verified signed image.
  192 native controller attach/reject/cleanup cycles pass. Four live KS interfaces
  answer pin and physical-connection queries, but **ToneNari MMDevice endpoint creation
  still fails** (`driver-final-v3`, exit 1). Actual WaveRT PCM remains untested.
  Secure Boot is disabled and the scoped test certificate remains installed only in
  this Windows VM; the pre-change disk/UEFI/TPM recovery copy was verified offline.
  See [WaveRT preflight](../../docs/performance/2026-09-07-windows-wavert-preflight.md).
- Windows ARM64 native engine build, 48 focused tests and Clippy passed
  (`windows-native-v8`). Primary-output native demand now paces the graph.
  Interactive tests passed four actual graph output attachments/removals plus
  120 capture/render/teardown cycles (`windows-audio-v6`, 86.34 seconds), exact
  0.004/0.002 peaks and handles stabilized at 154. Last-30-cycle private committed
  memory range was 140 KiB. These are bounded observations, not long endurance,
  multi-device drift correction or signed-driver qualification.
  See [Windows render-demand record](../../docs/performance/2026-09-06-windows-render-demand.md).
- Linux now uses native `pw_stream` PCM buffers and a `pw_filter` graph callback,
  not the earlier FIFO/timer transport. Ubuntu's native matrix v5 passed reverse
  routing at 2/8/16/32/64 channels, selected 64→16 routing, summation, half-duplex,
  and 24 endpoint teardown cycles. Tail RSS range was 8 KiB; native objects were
  gone after the owned engine stopped. This bounded check is not a general
  memory-leak guarantee. The later application-capture case in matrix v6 failed
  at process discovery and remains open. Do not call the entire matrix complete.
- Ubuntu native PCM ring ASan/UBSan test passed. Debian native build, Clippy and
  41 focused tests passed; Debian native live audio is not yet requalified.

The older failures below are retained as history, not relabeled as passing.

| Check | Actual result |
| --- | --- |
| Lab runner/control/guest helper tests | 22 passed on local macOS; includes completion markers, no-overwrite evidence, Linux HOME path regression, old/new pw-cat raw PCM options and transient-engine ownership/cleanup |
| Compose config, shell syntax, actionlint | Passed |
| macOS desktop compile after portability fixes | Passed |
| macOS vendored adapter regression tests | 5 passed |
| Debian 13 ARM64 container | Full workspace + UI build passed after fixes |
| Ubuntu 24.04 ARM64 container | Full build; 55 workspace Rust + 5 adapter + 62 UI tests passed |
| Kali rolling ARM64 container | Full build; same 122 tests passed |
| Arch x64 container on ARM Mac | Blocked: pacman seccomp under amd64 emulation; not bypassed |
| CI native Windows x64/ARM64 and Linux x64/ARM64 | Workflow added and linted; not executed remotely |
| UTM | 4.7.5 installed; scripting create/export validated |
| Linux VM media | Debian, Ubuntu, Kali, Arch ISO downloads matched official HTTPS SHA-256 manifests |
| Linux VM bundles | Four bundles registered in place on the selected external volume; Debian and Ubuntu installed and running at latest check, installation media ejected; Kali and Arch not installed |
| Kali installer display | Both graphical and text installer boot attempts show a black/white incomplete display with `virtio-gpu-pci`; no installer form reached and disk remains blank. Installation is not verified |
| Debian guest | Debian 13.6 ARM64, kernel 6.12.107+deb13-arm64; GNOME logged in; optional SSH server and usage survey excluded |
| Debian guest build/test | Full workspace/UI build; 55 workspace Rust + 5 adapter + 62 UI tests passed; embedded-frontend desktop build also passed |
| Debian desktop launch | Transient user process launched; journal contains EGL/DRI2 warnings; screen locked before visual inspection, so GUI behavior is not verified |
| Debian user audio session | PipeWire 1.4.2 and WirePlumber active; transient non-root ToneNari service created virtual source/sink; loopback API responds |
| Debian live stereo continuity | **Failed**: input/output meters respond, but captured active fraction 0.423634; longest internal near-silent run 26,173 frames (545.27 ms at 48 kHz). Not a production-quality audio pass |
| Ubuntu guest build/test | Ubuntu 24.04.4 ARM64, kernel 7.0.0-31-generic; full workspace/UI build and 122 tests passed; no desktop app interaction qualification |
| Ubuntu user audio session | PipeWire 1.0.5 and WirePlumber active; transient non-root engine created virtual source/sink and responded over loopback; test engine stopped after measurement |
| Ubuntu live stereo continuity | **Failed** after correcting the test CLI compatibility: active fraction 0.503943, longest internal near-silent run 1,266 frames (26.375 ms at 48 kHz). Input/output meters reported the expected 0.125/0.0625 peaks |
| Linux FIFO correction | Oversized FIFO reads discarded queued PCM; bounded reads now preserve bursts. Ubuntu Rust library + binary tests: 60 passed; Debian engine binary tests: 5 passed; Ubuntu engine Clippy including tests passed |
| Ubuntu corrected live continuity | Same-condition 3-run median active fraction 50.5782% → 99.4063%; median maximum gap 26.521 ms → 2.500 ms. **Still failed in 2/3 trials**, not a product pass. Detailed before/after report linked below |
| Native process contract | Actual OS DSP calls now validate cycles and report transferred frames/lifetime. macOS focused 37 tests; Ubuntu focused 43 tests, native build and Clippy passed. 10,000 portable DSP cycles had zero measured heap allocations/frees |
| Worker shutdown ownership | Cancellation/join/resource-Drop unit test passed. Ubuntu live SIGTERM stopped the owned transient engine and removed its actual endpoint FIFO/directory without test-side deletion |
| Contract migration live probe | Ubuntu worker reports running, engine_timer, clock_not_synchronized and real starvation counts. 3/3 continuity trials still failed (368/240/112-frame maximum gaps). No clock-synchronization or audio-quality pass claimed |
| Windows buffer-ready output | Source changed from 2 ms polling to WASAPI event-driven output. MSVC-target Clippy passed; native execution/performance not measured |
| Windows VM media | Windows 11 25H2 ARM64 Korean v2 ISO downloaded; SHA-256 matches Microsoft's official value |
| Windows VM | ARM64, 4 cores, 8 GiB RAM, 96 GiB NVMe; TPM 2.0 and UEFI key preload configured, Secure Boot not verified inside Windows. Desktop reached; QEMU-GA and vdservice both running after Guest Tools installation. Guest registry confirms ARM64 although agent CMD runs under AMD64 emulation. Native build/audio tests not run; Visual Studio Installer is waiting for Microsoft license consent |
| Live guest plugin editor/driver qualification | Not run |

The first Debian ARM64 compile exposed 11 `i8`/`c_char` type errors in the VST3/CLAP adapters.
Narrow vendored fixes use the ABI types, preserve UTF-8/TUID bytes, and retain upstream licenses.
A second compile found the macOS-only `is_success` helper compiled unused on Linux; its `cfg`
now matches its callers. The full Debian build then passed. These are correctness fixes, not
performance claims; the cold/warm build timings are not a controlled performance comparison.

Kali's minimal image needed an explicit trusted CA bundle path before ca-certificates was
installed. Its container uses the official fixed mirror and archive key; TLS/signature verification
remain enabled. `apt-get update` now fails immediately on index errors instead of cascading into
misleading missing-package errors.

VM disk and firmware exports were byte-compared with their newly created, unbooted internal
staging copies before those staging copies were removed. The external bundles are retained.
The final external configs have HDA audio, no clipboard sharing and no directory sharing.
UTM's first-launch sheet was dismissed and all four external bundles were registered by opening
them in place. `lsof` confirmed QEMU opened the external disks and media. Debian installation
has since completed and its ISO was ejected. This storage check alone is not an audio pass.
No host audio service or user graph was reset. Guest tests use a separate transient engine.

The stereo probe sends known 440/880 Hz signals exclusively to the named ToneNari virtual sink
and records its virtual source, without changing the default hardware devices. A first narrow
signal-presence check passed; a follow-up continuity assertion exposed the silence above.
The checked-in probe now fails on those gaps. This is a correctness baseline, **not** a
before/after performance claim or evidence of sample-accurate routing. The FIFO/timer Linux
transport still needs repair and callback/clock qualification.

Ubuntu's first probe failed before streaming because its `pw-cat` 1.0.5 does not accept
`--raw`. The probe now detects that option in `--help`: the
[1.0.5 implementation](https://raw.githubusercontent.com/PipeWire/pipewire/1.0.5/src/tools/pw-cat.c)
uses raw PCM automatically with the `-` filename, whereas newer versions advertise `--raw`.
After this compatibility correction the same Ubuntu guest produced an actual recording and
failed the continuity assertion above. This fixes the test harness, **not the engine**.
Debian/Ubuntu figures come from different guest runs; they are not before/after performance data.

The Linux development installer incorrectly used `$USER` as a directory fallback. It now uses
`$HOME`; a mocked installer regression test verifies its generated service path. This fix has
not yet been qualified as a persistent guest service install.

Detailed reports/log tails and media are in the configured external lab directory; its verified
volume identity/location are local-only in `.lab/config.json`. Do not substitute an internal
directory when the external volume is absent. Build images contain private source and remain local.

The later [FIFO correction and controlled continuity comparison](../../docs/performance/2026-09-06-linux-fifo-continuity.md)
records the actual engine fix, remaining failures and exact binary/probe hashes. The earlier
baseline failures above remain evidence and are not relabeled as passing. The native runner
now tests Rust binaries as well as libraries so OS-specific adapter tests are no longer omitted.
