# Windows audio: reuse the platform driver framework

Status: accepted; upstream ARM64 endpoint/stream smoke and diagnostic shared-PCM
bridge smoke passed. The opt-in Rust LAB adapter also passes real graph/PCM/meter
and removal tests. Configurable product integration and driver-isolation qualification
remain open. See the [Rust integration results](../performance/2026-09-07-windows-rust-bridge.md).

## Decision

Use Microsoft's SysVAD as the Windows PortCls/WaveRT framework instead of continuing
to reimplement its generic miniport lifecycle. Preserve the experimental MixRelay
driver and its failing endpoint-enumeration evidence for comparison. Do not replace
the product backend or advertise a working virtual device until native PCM checks pass.

The first baseline keeps upstream audio source and project files unchanged. A lab-only
`Directory.Build.props` selects the matching WDK/SDK NuGet packages already pinned to
10.0.26100.6584. Build the original MSBuild projects, retaining their compiler checks,
instead of assembling a second handwritten compiler/linker command line.

Source: Microsoft Windows-driver-samples revision
`197ba2156a60e2b76fcd4820bae594223e91a1e9`; WIL submodule revision
`3c00e7f1d8cf9930bbb8e5be3ef0df65c84e8928`.
Exact artifact/package hashes are in `infra/lab/sysvad-baseline.json`.

## Reuse boundary

- Retain the framework for PnP, power, PortCls registration, WaveRT stream lifecycle,
  position/packet reporting and notifications.
- Start with the upstream endpoints as a reference. Only after they enumerate and
  stream, reduce the endpoint tables and matching INF to the product's render/capture
  pair; do not remove arbitrary interfaces simply because the code looks larger.
- Replace the sample capture tone and render-data sink at their PCM boundaries with
  the MixRelay transport. Preserve sample-buffer format contracts until the transport
  explicitly handles conversion; never reinterpret integer PCM as floats.
- Keep graph routing, channel mapping, processors and UI in the shared user-mode engine.
  Windows driver reuse does not imply that Core Audio and PipeWire share a driver ABI.

The sample tone is an intentional baseline signal, never evidence of MixRelay routing.
Similarly, a successful SYS build or control IOCTL is not an MMDevice/PCM pass.

## License and provenance

The upstream repository uses MS-PL, not MIT. Retain its LICENSE and copyright notices
with copied sources and record product changes separately. Do not relabel upstream
code as independently authored MixRelay code. WIL retains its own license. The initial
reference source lives in the external lab volume and is not included in application
packaging. Distribution/licensing review remains necessary when importing product code.

## Validation gates

The [2026-09-07 native baseline](../performance/2026-09-07-windows-sysvad-baseline.md)
passed gates 1–4 at smoke-test depth: four render and four capture endpoints are
healthy, and three explicit WASAPI render/capture lifecycle cycles complete.
Capture is the sample tone; one discontinuity flag was observed per capture run.
This is not a continuity, loopback-routing, or endurance pass. The unchanged sample
passes Universal INF verification (`/u`), matching its project target; stricter
Windows Driver isolation verification (`/w`) fails and remains a product gate.

1. Build original ARM64 projects with the official WDK MSBuild integration.
2. Test-sign with the existing VM-local certificate; no new boot-policy exceptions.
3. Install the baseline under its distinct sample hardware ID, preserving the current
   MixRelay device. Record installed image hashes and enumerate native MMDevice endpoints.
4. Test explicit sample render/capture endpoints in the interactive Windows session.
   Do not select the user's hardware or capture unrelated applications.
5. Integrate product transport, then test known PCM in both directions, 1–64 channel
   mapping, supported sample rates, teardown and owner-process failure.
6. Requalify x64 separately, then release signing/HLK and endurance. ARM64 VM results
   do not certify native x64, other OSes, or physical-device clock drift.

This is a correctness/reuse change, not a performance claim. No functioning end-to-end
MixRelay Windows driver baseline exists yet for a comparable latency/CPU measurement.

References:

- https://github.com/microsoft/Windows-driver-samples/tree/main/audio/sysvad
- https://github.com/microsoft/Windows-driver-samples/blob/main/LICENSE
- https://learn.microsoft.com/en-us/windows-hardware/drivers/install-the-wdk-using-nuget
- https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools?view=vs-2022
