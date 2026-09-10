# Windows WaveRT preflight — 2026-09-07

The driver now loads in the authorized Windows VM. **Endpoint creation still fails;
this is not a product-audio pass.** Earlier preflight results are retained below.

## Implementation

- Product-owned PortCls render/capture miniports, bounded DMA work, QPC frame position,
  event notifications and lifecycle-only DPC draining. No sample tone or sample identity.
- Version 2 controller mapping: page-aligned zeroed locked pages, private kernel layout,
  bounded SPSC counters, wait-only event handles, and rejection of concurrent controllers.
- File cleanup and process-exit notification drain old DPC references before unmapping.
  The exit notification covers duplicated handles that outlive the mapping owner.
- PnP stop/removal marks an existing mapping offline and signals waiters, preserving its
  address until owner cleanup. Kernel address/length/clock state is never derived from
  user-writable header metadata.
- PCM copy planning rejects backward/out-of-window counters and overflow. Capture
  underflow is zero-filled; render overrun does not overwrite unread frames.

## Reproducible evidence

Windows 11 ARM64 VM, MSVC 14.44.35207, WDK ARM64 10.0.26100.6584, SDK 26100.
WDK package SHA-256:
`e705b2a63eab891def8f98087666f93e8f21da8e3b5def81a624b83fef5bdae9`.
The preparation script verifies this hash and Microsoft's Authenticode signature on
the INF validator. No PowerShell execution-policy override is used.

| Check | Result |
| --- | --- |
| Native kernel build/link (`windows-driver-v6`) | Exit 0, `/W4 /WX`, ARM64 `.sys`, stack protection, NX, CFG, integrity-check flag |
| WDK `InfVerif /w /v` | `INF is VALID` |
| Native shared PCM code test | 4,096 cases passed, 1–64 channels, four capacities, wrap/underflow/corrupt-counter cases |
| Same PCM code on macOS with ASan/UBSan | 4,096 cases passed, no sanitizer diagnostics |
| Driver loaded (v8 / signed-driver-v3) | Running, PnP error 0, installed image SHA-256 matches signed package |
| Native IOCTL attach/reject/cleanup | 192 cycles passed: 1–64 channels × 44.1/48/96 kHz; wrong ABI and concurrent controller rejected |
| Native KS interface discovery | Four present/active interfaces; each reports two pins; render/capture physical connections resolve |
| MMDevice endpoint creation (`driver-final-v3`) | **Failed, exit 1:** zero MixRelay endpoints; only existing HDA endpoints are present |
| Real WaveRT samples transferred | **Not run** |
| Kernel memory-lifecycle/Verifier/long endurance | **Not run** |

The external lab evidence directory contains `windows-driver-v6-20260907.log`.
Evidence SHA-256: `30ad8231b5b46887538c49ad4ad0ae41448bd0adc83817d3543f91c9708848a0`.
Native source archive SHA-256: `0700abbd2e10ff2fc8496b83e779839c43b89614dccf217bb9be069c61aa16b5`.
The 24 existing lab helper tests and `git diff --check` also passed.
Earlier v1/v2/v5 build errors are retained in distinct VM job directories, not overwritten.
This is a correctness change. There is no prior functioning product driver for a same-condition
performance comparison; CPU, latency and long-term pool measurements remain unmeasured.

## Authorization and stopping boundary

The user explicitly authorized test certificates, test-signing mode, reboot, and necessary
Secure Boot changes **only for the MixRelay Windows VM**. Its guest-reported UUID matches
the targeted UTM VM. Inspection found Secure Boot enabled and BitLocker protection status 0
(volume encrypted, protection off); no recovery key was read or logged.

After the user unlocked the host and confirmed continuation, VM-only signing and
installation proceeded. The Windows guest was gracefully shut down for a pre-change
disk/UEFI/TPM backup.
The APFS recovery copy's 42,799,333,376-byte disk, UEFI variables, TPM state and VM
configuration all passed byte-for-byte comparison while the VM was stopped.

Recovery copy: `/Volumes/Linear 1/test/mixrelay/recovery/windows-pre-testsigning-20260907`.
It contains the matching original VM configuration, disk, UEFI variables and TPM state.
Do not register or boot that same-UUID copy alongside the active VM.

Secure Boot was disabled through this VM's UEFI UI, without deleting keys or resetting
its TPM. `Confirm-SecureBootUEFI` reports false. BCD test signing is enabled and, after
reboot, the running kernel's `SystemCodeIntegrityInformation` reports options 2621959
with the TESTSIGN bit set. No host or other VM security settings were changed.

VM-local code-signing certificate `CN=MixRelay Windows VM Test 20260907`:
thumbprint `BF2F641C417A264E7F51FC75A91E8AA19A87674D`, expires 2026-10-07.
Its private key is non-exportable; only its public certificate was copied to this VM's
LocalMachine Root and TrustedPublisher stores. The certificate and test mode remain
enabled for the authorized driver lab, not production distribution.

The SYS and catalog signatures passed verification. DevCon updated only
`Root\\MixRelayVirtualAudio` / `ROOT\\MEDIA\\0000`; update exit 1 required a normal reboot.
Installed version 0.1.0.2 SYS SHA-256:
`4685752EB0DA45B0EAB5357D46E01102F386C413D7104C812C51419139DA9A3F`.
The running service path and hash were checked again after reboot.

Native testing exposed a lifetime bug: `PcRegisterSubdevice` names were stack-local
even though the API retains their storage for the device lifetime. v8 moves them to
stable module storage. Explicit KS categories and multimedia INF registration were
also added. These fixes are not claimed to resolve endpoint creation: both v7 and v8
still fail that integration assertion. `KsInterfaceProbe.cs` now distinguishes real
present/active interfaces and live pin queries from registry-only installation entries.
The remaining failure lies beyond interface publication; format negotiation and the
endpoint-builder topology contract require further investigation, not guessed success.

Evidence in `/Volumes/Linear 1/test/mixrelay/evidence`:
`windows-driver-v8-20260907.log`, `signed-driver-v3-20260907.log`,
`update-driver-v3-20260907.log`, `driver-final-v2-20260907.log`,
`driver-final-v3-20260907.log`, and `ks-probe-v2-20260907.log`.
The 192 attach cycles validate normal controller cleanup, not PCM delivery, process
death, duplicated-handle ownership, kernel-pool leaks or long endurance.

Separate VM stability observation: Windows logged bugcheck 0xD1 at guest time 09:50:53,
with WER identifying `viogpudo.sys`, before the first MixRelay driver installation.
The event and dump `C:\\WINDOWS\\Minidump\\090726-7062-01.dmp` were preserved; no causal
claim about MixRelay is made. No guest GPU/TPM policy changes were used to suppress it.

This remains a correctness investigation: no performance improvement is claimed and
there is no successful end-to-end baseline for comparable latency/CPU measurements.

Still missing: full channel/name reconfiguration and format negotiation (intersection currently
defaults to stereo/48 kHz), native x64 qualification, v2 Rust consumer, real PCM integration,
kernel lifecycle tests, production signing/HLK, multi-device ASRC and long endurance.
Other OS work is not certified by these Windows checks.

## Reference contracts

- [WDK downloads and VS compatibility](https://learn.microsoft.com/en-us/windows-hardware/drivers/other-wdk-downloads)
- [Process notification lifetime and integrity flag](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/nf-ntddk-pssetcreateprocessnotifyroutineex)
- [User mapping must be unmapped in its original process](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-mmunmaplockedpages)
- [WaveRT SetState runs at PASSIVE_LEVEL](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/portcls/nf-portcls-iminiportwavertstream-setstate)
- [Subdevice name storage lifetime](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/portcls/nf-portcls-pcregistersubdevice)
- [EndpointBuilder discovery and topology contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/audio/audio-endpoint-builder-algorithm)
