# Windows ARM64: original SysVAD baseline

Date: 2026-09-07. This is a correctness/reuse baseline, not product certification.

## Result

In the same Windows 11 ARM64 VM where the experimental MixRelay driver creates
zero MMDevice endpoints, unmodified Microsoft SysVAD audio source creates **four
render and four capture endpoints**, all PnP healthy. Explicit sample endpoints
also complete three WASAPI open/start/transfer/stop/release cycles.

This establishes a working reference for the PortCls/WaveRT framework. It does not
identify the exact defect in the experimental miniport, nor implement the MixRelay
transport in SysVAD. The sample capture tone is not routed audio from the engine.

## Reproducible inputs

- VM UUID: `712A936E-9D91-4455-A8A9-A1F65EB60719`; Windows 11 ARM64 build 26200.
- Upstream: `microsoft/Windows-driver-samples` commit
  `197ba2156a60e2b76fcd4820bae594223e91a1e9`.
- WIL: `3c00e7f1d8cf9930bbb8e5be3ef0df65c84e8928`.
- Fixed WDK/SDK NuGet version: `10.0.26100.6584`; archive/package SHA-256 values
  retained in `infra/lab/sysvad-baseline.json`.
- Visual Studio Build Tools 17.14.39, MSBuild 17.14.51, MSVC 14.44.35207 ARM64.
- Official DriverKit BuildTools, ARM64 ATL and Spectre library components installed.
- Original EndpointsCommon, TabletAudioSample and KeywordDetectorAdapter MSBuild
  projects; Release/ARM64. No audio C++ or project-file patches. The sole extracted
  source-tree override is the lab's `Directory.Build.props` selecting toolchain paths.
- Upstream MS-PL and WIL licenses/notices retained with external reference sources.

## Native results and retained failures

| Job | Result |
| --- | --- |
| sysvad-prepare-v1 | Exit 0; official SDK archives hash-checked and extracted |
| sysvad-build-v1 | Failed MSB8020: missing WDK MSBuild platform toolset |
| sysvad-toolset-v1 | Exit 0; official VS components installed; no reboot required |
| sysvad-build-v2 | Exit 0; upstream library and kernel driver built |
| sysvad-package-v1 | Failed stricter `/w` INF verification: 1320/1321 and 2663 |
| sysvad-package-v2 | Exit 0; upstream Universal `/u` validation, catalog generation, signing and member verification passed |
| sysvad-install-v1 | Exit 0; separate sample device installed, service running |
| sysvad-verify-v1 | False failure: test incorrectly expected SYSVAD in localized display names |
| sysvad-verify-v2 | Exit 0; test corrected to native parent identity; render 4, capture 4, all healthy |
| sysvad-probe-build-v1 | Exit 0; native ARM64 WASAPI smoke compiled with `/W4 /WX` |
| sysvad-audio-v1 | Exit 0; three explicit sample render/capture lifecycle cycles passed |

The KeywordDetectorAdapter build emitted SDK MIDL warning C4819 for code page 949;
it was not suppressed. The original Universal INF passes `/u`; `/w` errors concern
HKCR keyword COM registration isolation and missing `MsApoFxProxy.Registration`.
These stricter product requirements remain open. No `/samples` exclusions or
individual-error suppression was used, and failed logs were not overwritten.

The installed sample is `ROOT\MEDIA\0001`, hardware ID
`Root\sysvad_ComponentizedAudioSample`, service `sysvad_componentizedaudiosample`.
Its installed image matches the test-signed package SHA-256:
`7C2E6495B7C63197850D65BCAE5DE87F8C7729CA568D393CC63B0BCE8E6D7988`.
The existing MixRelay device `ROOT\MEDIA\0000` was preserved.

## Stream smoke scope

The interactive, non-administrator probe opens only the exact observed sample
Speakers and Internal Microphone Array - Front endpoint IDs, after checking their
parent is `ROOT\MEDIA\0001`. No default device, physical microphone, or unrelated
application is selected. Each flow runs for about 1.5 seconds, repeated three times.

| Cycle | Render frames (48 kHz, 2 ch, 32 bit) | Capture frames (48 kHz, 1 ch, 32 bit) | Nonzero capture bytes | Capture discontinuities |
| --- | ---: | ---: | ---: | ---: |
| 1 | 72,480 | 71,520 | 184,760 | 1 |
| 2 | 72,480 | 72,000 | 186,000 | 1 |
| 3 | 71,520 | 71,040 | 183,520 | 1 |

Both audio clocks advanced in every cycle. Render intentionally submits silence;
capture reads the sample's generated tone. The test checks resource lifecycle,
frame transfer and signal presence, not signal fidelity or continuity. It does
not distinguish startup from later discontinuities, so the flags above are not
dismissed as harmless startup events. COM interfaces, mix formats and started
clients have scoped cleanup; three cycles are not a memory-leak guarantee.

## Evidence and next implementation boundary

Guest job directories/logs are preserved under `C:\MixRelayLab`. Host copies live
under `/Volumes/Linear 1/test/mixrelay/evidence/sysvad-baseline-20260907`.
Local lab guard tests: **29 passed**. No unrelated source was staged or committed.

Use the accepted framework reuse boundary in ADR 0011: reduce paired endpoint
tables/INF deliberately, replace generated capture tone and render sink with the
MixRelay PCM transport, and handle sample format conversion explicitly. Then prove
known PCM routing, 1–64 channel mapping, rates, process death and teardown. Requalify
x64, driver isolation, release signing/HLK and endurance separately.

VM-only test signing and the existing scoped certificate were reused. No additional
boot/security exception was added. The sample remains installed for comparison.
The temporary interactive test task is removed after completion; evidence remains.

No latency/CPU improvement is claimed: the old MixRelay implementation has no
working end-to-end device baseline for a same-condition performance comparison.
The 0-versus-8 endpoint result is a functional comparison, not a benchmark.
