# Product implementation gates — 2026-09-07

This is a feature-preservation checklist, not a completion claim. A portable
contract, a native implementation, and a live qualified OS path are different gates.
The canonical owner, issue, per-OS implementation and acceptance state is the
[feature parity matrix](feature-parity-matrix.md), generated from
[its JSON source](feature-parity-matrix.json). This document summarizes boundaries;
it must not become a second, independently maintained completion matrix. Earlier
live OS evidence remains in [lab validation](../infra/lab/VALIDATION.md).

## Boundaries to preserve

Latest control-plane slice: [durable endpoint settings](validation/2026-09-07-endpoint-persistence.md).
Native follow-up: [HAL reconnect fix and Windows descriptor profile](validation/2026-09-07-native-endpoint-followup.md).
Hardware I/O follow-up: [Linux device I/O and shared selection contract](validation/2026-09-07-linux-device-io.md).
Recovery follow-up: [Linux independent device recovery and bounded handoff](validation/2026-09-07-linux-device-recovery.md).
Current native device integration: [macOS / Windows / Linux device parity](validation/2026-09-07-native-device-parity.md).
Windows OS invalidation gate: [failed PnP attempts and restored VM state](validation/2026-09-07-windows-device-pnp.md).
macOS CoreAudio and Windows WASAPI now have real hardware input/output implementations,
not just device discovery or capability flags. macOS owned aggregate removal/reappearance
passes four C and four Rust integration cycles. Windows passes injected reopen cycles
using real WASAPI clients, but actual LAB parent invalidation is still blocked by
pending-restart driver state. Neither establishes physical multichannel hardware parity.
Linux selected-device reappearance now restores the same graph without reopening
healthy streams or resurrecting deleted nodes. Native Ubuntu fixture acceptance is
separate from physical-hardware and other-OS recovery qualification.
Ubuntu native fixture PCM now passes 2/8/16/32/64-channel device routing, 64→16
channel selection, live gain, meters and teardown. This is not physical multichannel
hardware or all-distribution qualification.
The first macOS installed restart failed because HAL did not resubscribe after
engine exit. The corrected installed HAL passes four engine restarts, native name /
64/16 restoration and a subsequent stereo PCM smoke (281,600/281,600 samples match).
Windows 64/16 native descriptors compile on ARM64, but are not installed
or advertised as runtime capability.
The UI now consumes actual adapter configuration policy and retains accepted
endpoint settings and deleted-side state across engine restarts. Windows variable-channel device
implementation remains a separate, unfinished native driver gate.

| Boundary | Existing implementation / regression protection | Remaining product gate |
| --- | --- | --- |
| Graph editing | Node/edge deletion, fan-in, reconnect, context-menu selection, undo/retry and queued revisions in `apps/control-ui/src/core/audio-graph` and `features/routing-workspace`; focused UI regression evidence is tracked in the canonical matrix | Durable multi-graph work and native pointer interaction, bulk cable previews, platform shortcuts and requested visual states require their own acceptance, not an audio-backend pass |
| Channel routing / DSP | Graph compiler, explicit channel mapping, summation, one-way/duplex endpoints, gain, compressor/limiter and meter rendering; Linux owned fixture PCM covers 2/8/16/32/64 widths and 64→16 mapping | Hardware 64→16 and 64→64 routing must work through each actual backend, not just the compiler or synthetic callback layouts |
| Control / ownership | Typed graph/processor/editor API; stale revision handling; pending/retired processor ownership; native device owners recover without requiring a graph rewrite | Actual device/default transitions, permanently blocked vendor calls and long-running shutdown/reconfiguration remain separate gates |
| Native processors | Real format adapters, isolated worker processing and same-worker native parent windows in `crates/processor-host`; state/parameter follow-up has its own matrix owner | Installed AU/AUv3/VST3/CLAP/LV2 audio, editor parenting/lifetime, state round-trip, authorization and reopening need per-OS native qualification; HTTP editor route tests are not this proof |
| Windows transport | Separate host-testable v2 wire/layout contract and Win32 mapping owner; exact LAB identity opt-in retained | Product endpoint descriptors/names/channel selection and deployment are still missing; a general layout validator does not create a 64-channel Windows audio endpoint |

## Source-declared backend capabilities, not live certification

| Current engine adapter | Hardware input/output | Virtual / app capture | Live format / drift correction |
| --- | --- | --- | --- |
| macOS (`src/main.rs::macos::adapter_profile`) | true / true | true / true | true, limited / false |
| Linux (`src/linux.rs::adapter_profile`) | true / true | true / true | false / false |
| Windows (`src/windows.rs::adapter_profile`) | true / true | LAB opt-in only / true | false / false |

In particular, device discovery in the desktop does not prove that these engine
adapters can capture/render that device. Debian, Ubuntu, Kali and Arch need actual
session/backend evidence independently; an OS label or a container build is not audio proof.
macOS advertises 44.1/48 kHz and 64/128/256/512-frame blocks, but application capture
is 48 kHz only and attached hardware nodes currently reject a session-rate change.
Windows/Linux remain at their fixed engine format. All three explicitly report no
MixRelay sample-rate conversion or independent-device drift correction.

## Current completed slices and remaining qualification

The [runtime integration record](validation/2026-09-07-runtime-completion.md)
supersedes older omission notes for application retracking, native close/normal-exit
state checkpoints, fixed-positive virtual status, and LV2 portable state. Windows
now retains a stable executable-path capture owner across relaunch, with real
nonzero WASAPI PCM qualification in v15; Ubuntu's isolated PipeWire v7 fixture
similarly recovers nonzero PCM without graph reapply. These are native fixtures,
not every DAW, distribution, or production virtual-driver configuration.

The native close observer and orderly shutdown checkpoint retain the correct
worker/controller owner, preserve live presets during structural edits, and reject
failed persistence. [Actual isolated Apple AU state qualification](validation/2026-09-07-native-au-checkpoint-qualification.md)
covers new-worker restore, Undo, disk persistence and fresh-controller recall.
LV2 portable opaque properties and control ports also pass a native fixture;
file assets and nested URID containers remain explicitly unsupported. None of
these tests qualifies native graphical editor gestures or crash/power-loss durability.

## Remaining implementation and native acceptance

1. Windows worker-retention and quarantined-mapping recovery are implemented:
   12 fixture failure/edit/removal cycles and six real PCM cycles over two engine
   attachments pass. Actual kernel PnP hot-unplug/unload and startup underruns remain
   open. New native WASAPI input/output retry owners pass injected invalidation;
   actual PnP v2/v5/v6 attempts did not qualify recovery. The VM's exact LAB parent
   is enabled with healthy endpoints but has `DN_NEED_RESTART`; a controlled rerun
   after the restart/driver-state gate is required. See [transport evidence](performance/2026-09-07-windows-transport-recovery.md)
   and [PnP evidence](validation/2026-09-07-windows-device-pnp.md).
2. Replace sample-only Windows endpoints coherently across INF, PortCls pin/format
   tables, negotiated transport and user configuration. Preserve driver isolation,
   signing and session-authorization gates; never expose ABI limits as installed capabilities.
3. Qualify the implemented hardware I/O against actual devices, default changes,
   physical 64-channel routing and prolonged removal/reappearance. Implement
   explicit sample-rate/clock-domain negotiation, SRC and drift boundaries for
   independent devices. Native vendor calls that never return still need an
   isolation/containment design; retaining callback ownership is not leak freedom.
4. Linux registry parsing and stable application capture owners now handle native
   properties, Client-owned identity and stream replacement. Preserve the Ubuntu
   nonzero PCM fixture while qualifying real applications and each requested
   distribution; complete Kali/Arch VM installations.
5. Complete tracked plugin state/parameter work and qualify real plugin audio/editor
   lifetimes. Multi-bus selection, latency compensation, actual application-relaunch
   qualification and Linux/Windows source-muting policy remain separate gates. Complete native
   UI acceptance while retaining gain history coalescing, channel identity, meter
   mapping and deletion semantics.

Changes should run the affected native tests plus the portable graph/control and UI
regressions. Keep failed runs and ignored-test counts visible. Never replace an
unsupported native operation with silent success to make this checklist green.
