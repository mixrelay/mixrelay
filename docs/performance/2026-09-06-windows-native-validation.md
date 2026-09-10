# Windows ARM64 native validation — 2026-09-06

This is a partial implementation/validation record, **not Windows product certification**.
The later [render-demand validation](2026-09-06-windows-render-demand.md) supersedes
this record's graph-timer status and short lifecycle counts; the earlier results
and failures below are retained as history.
The guest is Windows 11 Pro ARM64 26200.8037 in UTM 4.7.5 on Apple Silicon.
Guest source, tools and logs are isolated in `C:\MixRelayLab`.

## What actually ran

- Official Microsoft-signed Build Tools 2022 17.14.39 installed, including ARM64
  MSVC 14.44.35207 and SDK 26100. `vswhere -products *` reports a complete,
  launchable installation with no reboot required. The default vswhere product
  filter excludes Build Tools and must not be used as an installation test.
- Rust 1.96.0 reports `aarch64-pc-windows-msvc`; `cl.exe` and `link.exe` resolve
  under `HostARM64\arm64`. Node is the hash-pinned ARM64 24.19.0 distribution.
- `windows-native-v1`: engine build, 41 focused Rust tests and Clippy passed
  natively. These are guest binaries, not a macOS cross-compilation result.
- `windows-native-v2`: engine build, 44 focused Rust tests and Clippy including
  test targets passed. One opt-in live WASAPI test was correctly ignored here.
- `windows-audio-v1`: the opt-in test ran in **interactive session 2** as the
  guest lab user, not guest-agent Session 0. It failed during initial capture
  activation with `0xc0000409`, fast-fail subcode `0x23`. No audio pass is claimed.
- After the approved UTM restart, `windows-native-v5` built the final source and
  passed **46 focused native tests plus Clippy including tests**, exit code 0.
- `windows-audio-v4` passed in interactive session 1: one complete warmup and
  12 further capture/render/teardown cycles. Every cycle captured 35,520–37,920
  frames and exact peaks **0.004 / 0.002**. Post-teardown handles were
  `[151,152,152,152,152,152,154,154,154,154,154,154,154]`.
- `final-state-v1` verified the one-shot task was removed and no owned engine
  or test process remained. Its exit code and both validation exit codes were 0.

## Defects found and repaired in source

1. A stack-backed activation `VT_BLOB` was stored in an owning `PROPVARIANT`.
   The pinned windows-rs 0.61.3 extension implements Drop using `PropVariantClear`;
   returning from activation could therefore attempt to free stack memory.
   `BorrowedActivationBlob` now suppresses that owning destructor and ties the
   blob's pointer to the borrowed parameter lifetime. A native regression test
   constructs/drops the wrapper without starting audio.
2. `include_descendants=false` incorrectly selected
   `EXCLUDE_TARGET_PROCESS_TREE`. That mode captures **other applications**, not
   the requested PID without its children. The adapter now rejects unsupported
   exact-PID requests; it never silently broadens capture. The lab test uses the
   current test process with its child tree and emits only a quiet owned signal.
3. Capture used 2 ms polling. It now waits on the WASAPI buffer-ready event,
   registered after Initialize and before Start. Timeout checks cancellation;
   it does not authorize reads. Stream/packet guards stop and release native
   resources on success and all error returns, including invalid packet pointers.
4. Capture COM initialization was unbalanced. Worker and inventory guards now
   balance their own successful initialization, without uninitializing an
   apartment owned by the caller.
5. The SPSC ring rounded free/available storage but not short caller slices.
   Both sides now transfer whole channel frames only, reject zero channels,
   preserve unread frames on overflow, and clear the remaining destination.
   Regression tests cover 2/8/16/32/64 channels, wrap and 10,000 concurrent frames.
6. Hardware sink writes previously reported the requested frame count even when
   the ring accepted less. They now report the actual transfer to the common DSP
   status path, counting a short write once rather than manufacturing success.

Microsoft references: [process selection semantics](https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params),
[official application-loopback implementation](https://github.com/microsoft/Windows-classic-samples/blob/main/Samples/ApplicationLoopback/cpp/LoopbackCapture.cpp).

## Rerun and remaining validation

The corrected ownership/selector code passes macOS-hosted Windows x64 MSVC-target
Clippy and actual Windows ARM64 execution. The first rerun
(`windows-native-v3`, `windows-audio-v2`) was interrupted; during it, UTM
guest-agent RPC and normal shutdown requests became unresponsive. A host sample
captured QEMU cleanup waiting in `spice_server_vm_stop → red_qxl_stop → read_safe`.
The Windows QEMU process eventually stopped; a new process was launched but UTM
remained in `starting`. With explicit user approval, UTM was restarted and the
previously running Debian, Ubuntu, Kali and Windows VMs were relaunched. Arch
remained stopped. Windows finished pending updates; host audio services/default
devices were not reset. The task was removed only after completed validation,
with its ownership checked. No boot/login trigger or stored password was created.

The first corrected live probe (`windows-audio-v3`) got exact audio in all 12
cycles, but failed its handle-count check (136→154). Its warmup had opened only
capture, so it compared cold render infrastructure against warmed infrastructure.
The test now measures every teardown after one complete capture AND render
warmup; it does not merely raise the threshold (the permitted increase was
reduced from 8 to 4). The final samples above stabilized at 154 rather than
growing per cycle. The failed log is retained, not relabeled as passing.

The passing opt-in test covers this guest boundary only: it is not a
long-duration leak, hardware continuity, latency, or full graph-routing qualification.

The graph still uses `TimerTimeline`; output and capture native events do not
establish one graph clock. Independent endpoint drift/SRC, format changes,
hardware capture, Windows x64 execution, and an installable signed virtual audio
driver remain unfinished. The driver header is not a working driver.

## Measurement limits

This change fixes ownership and contract correctness. No CPU/latency improvement
is claimed: the pre-fix live capture crashes before a comparable stream can run,
so a same-condition steady-state performance comparison is unavailable. The
alternative evidence is native unit tests, the captured Windows failure, API
lifetime review, and the explicit opt-in native regression probe. Cold versus
warm build durations are not an audio performance measurement.

Final checks after the repairs: Windows ARM64 native build, 46 focused tests,
Clippy and the separately enabled live test passed. Windows x64 target Clippy
also passed, but x64 execution remains unverified. Local macOS focused tests (39),
lab helper tests (21), and `git diff --check` passed.

Final source archive SHA-256:
`76d73ae40a239d2797c3c26f7eb2583a90908446a340cc58e7c1daa8fa96d7e1`.
Final guest engine SHA-256:
`b643bae093e110bfc52caa90bbb16f1a10872e39fdb88b8f3e98a5b11b0318be`.
Full logs are retained in the configured external lab's `evidence/` directory:
`windows-native-v5-20260906.log`, `windows-audio-v3-20260906.log`,
`windows-audio-v4-20260906.log`, and `windows-final-state-20260906.log`.
