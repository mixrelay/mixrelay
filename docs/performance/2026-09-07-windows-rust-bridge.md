# Windows shared PCM → Rust engine integration — 2026-09-07

## Result

The Windows Rust engine now has an opt-in adapter for the existing SysVAD LAB
transport. Actual WASAPI audio passes through `EngineGraphController`, the graph
compiler, `BlockProcessor` gain/channel routing and `WindowsExternalIo`, rather
than the earlier standalone C++ mapping controller. This is **LAB integration**,
not a released/configurable Windows virtual device or all-OS qualification.

Native ARM64 build `windows-rust-bridge-build-v9`: build, 20 engine-service tests
(11 library + 9 binary), and Clippy pass. Two older interactive WASAPI tests are
ignored in that unit run; they are not counted as passes. The separate interactive
integration job `windows-rust-bridge-audio-v9` returned 0 and proved:

- 48 kHz stereo LAB render → graph channel 1 → −6.020599913 dB gain → mono capture.
  Input +0.25 / −0.125 produces +0.125 capture, checked with 0.001 tolerance.
- Three 3-second capture cycles matched 141,442 / 142,498 / 142,018 frames.
  Each WASAPI capture still reported one discontinuity flag.
- The actual graph gain meter returned approximately 0.125. A graph revision was
  applied during audio without replacing the controller mapping or stopping PCM.
- Status reported `ready`, `device` clock authority and negotiated 48,000 Hz only
  after a successful native mapping/header handshake.
- Removing the graph released the mapping: a separate controller immediately
  reattached successfully while the Rust process still existed.

## Boundary and safety

`apps/engine-service/src/windows_transport.rs` owns the file, returned event handles
and pinned mapping. Closing the file invokes driver cleanup; it must not call
`UnmapViewOfFile` or `VirtualFree` on these driver-owned pages. Events drop before
the file. A single audio worker has mutable transport access, graph revisions share
the owner, and the existing reclaimer drops retired resource sets off the DSP path.

The adapter checks descriptor/header version, length, alignment, fixed negotiated
layout and monotonic bounded peer counters. Copies use private offsets, not mutable
shared-header offsets. Acquire/release publication matches the native interlocked
counter boundary. New native tests cover ABI size/offset/IOCTL and 768 wrapped
counter windows plus malformed, overflowing and regressing counters.

Driver render availability gates each 128-frame DSP cycle. Timeout alone cannot
manufacture a block. Status reads worker-published counters instead of reporting
zero when a transport lock is busy. The shared controller also gained an explicit
initial-document constructor: enabling a capability cannot invent a default device
node when the adapter actually started an empty processor. Its regression test
passes on both the macOS host and Windows VM.

Enable only for this installed LAB identity with `MIXRELAY_WINDOWS_SYSVAD_LAB=1`.
It accepts one duplex endpoint with node/endpoint ID `mixrelay.sysvad.lab`, display
name `MixRelay SysVAD Bridge LAB`, stereo render, mono capture and 48 kHz. The common
backend contract stays OS-independent. Unsupported names/channels/rates and mixed
independent native-device clocks are rejected before graph publication. Normal
Windows startup remains disabled for virtual endpoints; no production UI option,
device naming or 64-channel capability is falsely enabled by this test.

## Same-condition startup-buffer comparison

Same VM, installed driver, native probe v11, debug Rust build mode, 48 kHz, 128-frame
DSP, graph, live revision change and three 3-second cycles. Engine status sampled
approximately every 200 ms; the external probe does not invent driver counters.

| Case | Startup silence lead | Final underruns | Render overruns | WASAPI discontinuities |
| --- | ---: | ---: | ---: | ---: |
| audio-v6 / build-v6 | 0 frames | 3 | 0 | 1 per cycle |
| audio-v9 / build-v9 | 256 frames | 2 | 0 | 1 per cycle |

In both runs, all observed underrun growth occurred within about 0.43 seconds of
the first status sample. No later growth was observed over the remaining ~9-second
capture run. The added initial queue slack costs **256/48000 = 5.33 ms**; this is a
configured startup lead, not a measured end-to-end latency. A single 3→2 comparison
does not establish statistical performance improvement or zero-glitch behavior.
The lead is currently LAB-only. Remaining startup underruns, native capture
discontinuities, scheduling jitter and long-term independent-clock drift are open.

## Evidence and reproducibility

Guest: `712A936E-9D91-4455-A8A9-A1F65EB60719`, Windows ARM64, MSVC 14.44, Rust 1.96.
Existing signed SysVAD overlay driver and security settings were unchanged this
turn. Only explicit LAB endpoints were opened; no default-device fallback, unrelated
application capture, mixer-volume changes or driver installation occurred.

`SysvadBridgeProbe.cpp --external-engine` now only supplies/checks WASAPI PCM; it
does not map or process the shared ring. Driver counters come from Rust status.
`windows-rust-bridge-check.ps1` is the readable harness source; the checked-in CMD
contains the corresponding literal command. `render-rust-bridge-command.mjs` emits
it and a regression test checks exact equivalence. No execution policy or trust
store changes are made. The initial direct script-file launch did not execute under
the VM's default policy; later runs used the existing literal-command lab workflow.

Earlier failed test jobs are retained: stale graph revision, mismatched node and
endpoint IDs, and a missing cached process handle that made the test runner's exit
code unavailable despite passing PCM. The corrected runner caches the exact child
handle, checks its exit code and the real gain meter separately. Owned processes
are terminated in `finally`; successful runs remove the graph first and prove
mapping release. No process is selected for termination by image name.

Evidence: `/Volumes/Linear 1/test/mixrelay/evidence/rust-sysvad-bridge-20260907`, with
full guest jobs under `C:\MixRelayLab`. Host library tests: 11 pass; Clippy pass.
Lab suite: 38 tests including the final cleanup guard. `git diff --check`
passes. No files were staged or committed.

Final executable SHA-256:
`22229A1348E2B7759960CB59C2CB8E38040401FE336B7364A33E85BA75ACD33E`.
Cleanup validated the exact task action, resolved owner SID and completed state,
then removed only `MixRelayLabRustBridge`. A subsequent read-only check confirmed
that this task and the exact owned engine/probe executables were no longer running.
The installed driver, test sources, recovery copies and logs remain intact.

## Remaining product work

Configurable endpoint INF/tables, public naming/channel/rate controls, 1–64-channel
native matrices, authenticated non-admin service ownership, startup/continuity
fixes, hot-unload and long-run pool/handle tests, x64, `/w`, signing/HLK and normal UI
enablement. macOS/Linux implementations were not changed or newly qualified by this
Windows run. Resource cleanup smoke tests are not a no-memory-leak guarantee.
