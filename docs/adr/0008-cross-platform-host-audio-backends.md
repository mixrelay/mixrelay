# ADR 0008: Cross-platform host-audio backends

- Status: Accepted
- Date: 2026-09-04

## Decision

The graph schema, compiler, block processor, meters, processor hosting, and control protocol stay
portable Rust. Native resources are prepared behind two Core boundaries:

- `VirtualEndpointBackend` prepares graph attachments to virtual endpoints, separately
  from system-driver installation/deletion (see ADR 0010).
- `GraphResourceBackend` prepares the complete set of endpoint, device, and application-capture
  resources for a graph revision before its processor is published.

Prepared native resources are swapped together with the execution plan at a block boundary.
Anything whose destruction may block is placed on a retired list and reclaimed by a non-real-time
thread. The render path performs no allocation, process creation, registry lookup, COM activation,
module unload, or mutex acquisition.

## Platform implementations

### macOS

The AudioServerPlugIn remains isolated C. It communicates with the Rust engine through the existing
XPC/shared-memory transport. Application capture uses Core Audio process taps. The HAL bundle is a
system component and therefore uses a signed/notarized administrator installer plus launchd.

### Linux

The Linux backend targets PipeWire, not one distribution. Debian, Kali, Ubuntu, and other systems
use the same registry and stream semantics when PipeWire is present. `pw-dump` supplies discovery;
`libpipewire-module-pipe-tunnel` supplies graph-owned virtual sink/source nodes and application
capture taps. Bounded F32 FIFO rings absorb scheduler jitter and preserve whole interleaved frames.
Modules and FIFO paths have deterministic ownership and teardown.

PulseAudio-only systems are unsupported. The engine must report the missing capability instead of
silently using a second backend with different routing semantics.

### Windows

WASAPI supplies endpoint discovery and Windows process-loopback capture. Capture is activated with
`ActivateAudioInterfaceAsync` and a PID/process-tree selector. Every capture owns a worker and a
bounded lock-free SPSC sample ring; graph retirement joins workers off the audio thread.

A genuine Windows virtual render/capture endpoint requires a signed WaveRT driver. It cannot be
created by Tauri or by the user-mode Rust process. Until the product-owned driver is installed and
loaded, status is `not_installed` and virtual-endpoint apply is rejected explicitly. Development
installation uses `pnputil`; the user audio engine runs at logon in the interactive session rather
than as a Session 0 service.

## Channel model

One graph edge is one explicit channel route or a declared channel map; it is never an ambiguous
multi-channel “cable.” Endpoint channel counts remain dynamic from 1 through 64. Platform backends
must preserve the graph's discrete channel indices, and UI grouping must not change the stored
mapping.

## Consequences

The execution/clock contract and the limits of the current timer-driven adapters
are specified in [ADR 0010](0010-native-process-boundary.md). Portable compilation
alone does not certify native scheduling, continuity, or ownership equivalence.

- Distribution-specific code is limited to installation and dependency detection; Linux audio
  behavior depends on PipeWire capability, not `/etc/os-release` branding.
- Windows driver build/sign/install verification requires a Windows WDK host and cannot be proven
  by a Rust cross-check on macOS.
- Backends may advertise different capabilities, but unsupported behavior is always a structured
  apply error and never a silent no-op.
