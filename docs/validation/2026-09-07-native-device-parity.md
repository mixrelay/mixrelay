# Native device I/O and recovery integration — 2026-09-07

Status: **implemented native paths with limited acceptance, not all-OS completion**.
The [canonical feature parity matrix](../feature-parity-matrix.md) and
[JSON source](../feature-parity-matrix.json) own feature status, issue and assignment.
This report consolidates source inspection and the retained agent-run evidence;
writing this report did not rerun tests, change a VM, or change the host audio graph.

## Implementation versus acceptance

| Backend | Actual implementation | Evidence obtained | Not established |
| --- | --- | --- | --- |
| macOS | `MacDevices` prepares graph device owners; `MRDeviceIO` opens native CoreAudio IOProcs, copies supported channel layouts and supervises loss/reappearance | ASan/UBSan callback/lifetime suite; four C fixture and four Rust graph-owner removal/reappearance cycles on owned temporary CoreAudio aggregates | Physical microphone PCM fidelity, physical USB unplug, real default switching, physical 64-channel hardware, vendor teardown hangs |
| Windows | Native `windows_input.rs` and `windows_render.rs` own WASAPI capture/render clients; `device_worker::supervise` retries activated clients without requiring graph edits | Four injected input and four injected output reopen cycles using real WASAPI clients; isolated Windows source regression; read-only actual default-role inventory | Actual endpoint/parent PnP invalidation, physical hardware PCM through an unplug cycle, x64 Windows, permanent COM/driver blocking |
| Linux | Exact PipeWire target selection, independent input/output recovery, generation-tagged RT handoff and control-thread retirement | Ubuntu ARM64 owned source/sink PCM for 2/8/16/32/64 channels, 64→16 channel 13/33 mapping, gain/meter/recovery and repeated teardown | Physical 64-channel hardware, whole PipeWire restart, each requested distribution, long-duration independent-clock behavior |

Current adapter profiles correctly declare `hardware_input: true` and
`hardware_output: true` on all three platforms. A declared capability describes an
implemented path, not success against every device. Input/output discovery alone
was not counted as an implementation.

Relevant source:

- macOS: `apps/engine-service/src/main.rs`, `macos_devices.rs`,
  `macos_devices_tests.rs`, and
  `native/macos/engine-service/src/MRDeviceIO.c`.
- Windows: `apps/engine-service/src/windows.rs`, `windows_input.rs`,
  `windows_render.rs`, and shared `device_worker.rs`.
- Linux: `apps/engine-service/src/linux.rs`, `linux_devices.rs`,
  `linux_native.rs`, `native/linux/pipewire/MRPipeWire.c`, and
  `crates/audio-io/src/recovery.rs`.

## macOS evidence and ownership limits

The independent review used uniquely owned temporary aggregates built from the
existing MixRelay virtual endpoint. It did not change system defaults, hardware
rates, microphone permissions, the installed engine or the user's graph.

`bash scripts/check-macos-device-io.sh` completed with exit 0. Its C17 build enables
`-Wall -Wextra -Werror -fsanitize=address,undefined`. The final suite reports **384
callback/epoch/layout ownership cases**, covering 1/2/8/16/32/64 channels,
interleaved/planar and mixed native stream widths, stale input discard, output
re-prime, invalid-layout silence latching, teardown failure/retry and injected
default-selector changes in both directions.

The `--live` run also completed with exit 0: four actual C aggregate removal/arrival
cycles resumed input/output callbacks, followed by four Rust integration cycles
retaining graph owners and offline gain edits. The Rust native test was explicitly
run and passed; it remains ignored by default. Ordinary focused Rust tests passed
2/2. The final added mixed-stream unit case was run after live acceptance without
a production-code change. The review's integrated Clippy attempt encountered
concurrent graph-catalog diagnostics; it is not represented as an integrated pass.

Failed `AudioDeviceDestroyIOProcID` retains the registration and callback storage
until successful unregister or confirmed object deletion. Native shutdown waits
at most two seconds before quarantining a self-owning worker/context; it does not
free storage still reachable by a callback or create another retry owner. This is
a use-after-free containment policy, **not a guarantee of zero retained resources
when a vendor call permanently blocks**. Actual default selection was fault-injected,
not changed against the user's system. The live aggregate is stereo: synthetic
64-channel layout tests do not qualify physical 64-channel audio.

Retained review evidence:
`/Volumes/Linear 1/test/mixrelay/evidence/native-device-parity-20260907/macos-review/result.md`.
It records command results, callback counts, isolated native build directories and
the scoped no-surviving-fixture check.

## Windows evidence, failures and current blocker

Injected recovery recreates actual WASAPI clients, retaining graph-side owners.
Failed render owners cannot emit leftover demand as a valid native clock epoch.
These checks establish retry behavior but do not prove that Windows PnP delivers
the expected invalidation for the installed driver.

The [Windows PnP report](2026-09-07-windows-device-pnp.md) retains the failed attempts:

- v2 endpoint-only disable was accepted by Windows but did not invalidate existing
  SysVAD sessions within 15 seconds; both direction tests failed.
- v3/v4 stopped at the safety guard because LAB endpoints held all six defaults.
- After scoped authorization, v5 parent disable failed with `0x80041001`.
- v6 native `pnputil /disable-device` returned 50, reporting a prior operation
  awaiting restart and an unsupported request. No reconnect was credited.

Independent final inspection found the exact LAB parent enabled, `ProblemCode=0`,
`ConfigFlags=0`, all original **8/8** endpoints healthy and all six default-role
UIDs unchanged. `DevNodeStatus=0x0180210B` still contains `DN_NEED_RESTART`.
No reboot or security-policy change was performed by this acceptance run. Actual
PnP acceptance remains blocked pending VM restart/driver-state diagnosis and a
complete rerun, not waived because injected recovery works.

The isolated Windows snapshot passed **76 tests**, with **7 interactive tests
ignored** in the ordinary suite, plus Clippy with warnings denied. The actual
interactive default-role inventory was separately run and passed. Concurrent
root integration changes require their own synchronized validation; this result
must not be reused as proof that every later source change was tested.

Ordinary stop/retry workers join, but a permanently blocked initial synchronous
COM/driver call cannot safely be cancelled. After the initial 10-second timeout,
the current startup path can detach a self-owning worker. Containment/brokering
for that failure is still a product gate, not a no-leak result.

Evidence:
`/Volumes/Linear 1/test/mixrelay/evidence/native-device-parity-20260907/windows-pnp/`.
The detailed report records failed logs, final roles/flags/endpoints, regressions
and owned scheduled-task cleanup. Existing Windows virtual transport acceptance
is separate from these hardware-client tests. The installed virtual driver remains
the fixed **2-render / 1-capture LAB** profile; compiled 64/16 descriptors are not
installed product configuration or dynamic endpoint support.

## Linux evidence retained unchanged

[Linux device I/O](2026-09-07-linux-device-io.md) and
[Linux device recovery](2026-09-07-linux-device-recovery.md) retain native Ubuntu
PCM and recovery results. Independent loss/return preserves healthy stream serials,
gain edits and graph revisions, discards prepared stale input PCM, and does not
resurrect deleted nodes. Recorded v3 recovery observations were 132–355 ms; this is
an acceptance measurement, not a cross-platform speed comparison.

The same 32-cycle, 550-ms-settling memory workload failed before the native PCM
slab change: final RSS 111,912 KiB and post-warmup range 11,868 KiB. After explicit
`mmap/munmap` storage, v6 finished at 57,236 KiB with a 1,612-KiB range; unchanged
v7 repeated at 57,052 KiB with the same range. The paired final-RSS reduction is
54,676 KiB (48.9%). Failed runs remain retained. This bounded workload is not a
universal leak-free or endurance guarantee, and its numbers are not applied to
macOS or Windows.

## Residual product gates

1. Actual physical multichannel interfaces, 64→16/64→64 PCM, default changes and
   prolonged unplug/replug must be qualified separately on each OS.
2. macOS supports 44.1/48-kHz session settings but application capture is 48 kHz
   only; hardware-attached session-rate changes are rejected. Windows/Linux remain
   fixed-format. MixRelay SRC and independent-device drift correction are absent
   from all three capability profiles.
3. Permanent vendor/COM blocking requires a resource isolation strategy beyond
   preserving callback ownership. Sanitizers and bounded normal teardown do not
   close this gate.
4. Windows product-driver configuration/deployment, true PnP acceptance and x64
   qualification remain open. Ubuntu evidence does not certify Debian/Kali/Arch
   or a different display/audio session.
5. Application stream retracking, source-muting parity, installed plugin
   audio/editor/state/latency and native graph UI acceptance are separate issues
   in the canonical matrix. This hardware-device slice does not close them.

No all-OS completion, physical 64-channel certification, global zero-underrun result
or unconditional memory-leak freedom is claimed.
