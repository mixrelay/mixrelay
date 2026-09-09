# ADR 0005: Treat Loopback and Audio Hijack parity as the product baseline

## Status

Accepted

## Context

Rogue Amoeba splits its audio product into two adjacent models:

- Loopback configures persistent virtual devices as sources, output channels,
  and monitors. It includes application and physical-device capture,
  nested virtual devices, per-source and per-monitor level controls, explicit
  channel wiring, and up to 64 channels.
- Audio Hijack provides the free-form block graph: sources, processors, meters,
  recorders, device output, live streaming, broadcasting, scripting, and Audio
  Unit effects.

ToneNari would be a lower-value substitute if it implemented only the Loopback
routing subset. Product parity therefore means the union of both products,
while the implementation remains independent.

## Decision

The following inventory is the compatibility floor, not an aspirational list.
Every item must have a stable node type identifier, versioned parameters,
capability discovery, deterministic validation, unit tests, and a Storybook
state inventory before it is considered supported.

### Host audio endpoints and routing

- Persistent, independently enabled virtual render/capture endpoints which the
  operating system exposes to every compatible application
- Duplex, render-only, and capture-only endpoint specifications with stable IDs,
  channel layouts, sample rates, and automatic one-to-one mapping presets
- Application, special/background process, system-audio, physical-device, and
  other ToneNari virtual-device sources
- Multiple monitor devices, including partial channel monitoring
- Automatic mapping plus arbitrary explicit channel routes
- At least 64 channels per virtual device; limits come from capabilities rather
  than hardcoded machine assumptions
- Per-source, per-monitor, and device-level enable, mute, gain, and meters
- Application `mute while captured` policy
- Nested devices, disconnect recovery, clock-domain adaptation, and persistent
  selectors based on bundle IDs and Core Audio UIDs

### Sources

- Application/process audio, all system audio, physical input devices, virtual
  endpoint render streams, and file playback

### Processing and utility nodes

- Gain/volume, balance, pan, channel operations, mono enhancement, bass/treble,
  low-pass, 10-band EQ, parametric EQ, and simple compressor
- Magic-boost style loudness processing, ducking/sidechain, mixer, input switch,
  sync/delay, and time-shift buffer
- Declick, dehum, learned denoise, speech denoise, and FIR convolution
- Peak/RMS/LUFS meters and waveform/spectrum inspection
- AUv2/AUv3 on macOS, VST3 and CLAP on every desktop target, and LV2 on Linux. Third-party
  processing is isolated so a plug-in failure cannot corrupt the graph controller.

### Outputs and operations

- Physical output and ToneNari virtual-device output
- Recording with WAV, AIFF, FLAC, ALAC, AAC, and MP3 targets
- RTMP live stream and Icecast/Shoutcast broadcast targets with metadata
- Local transcription, where the selected model is available
- Presets, reusable graph templates, import/export, global hotkeys, schedules,
  event automation, and Shortcuts-compatible control
- Background execution, menu-bar control, launch-at-login, and restart recovery

### ToneNari extensions beyond parity

- One engine graph with separate virtual-device and processing-session UI
  projections, so each workflow stays predictable
- OBS-oriented templates and named multi-track buses
- Authenticated LAN control from a phone with no cloud dependency
- Versioned HTTP/WebSocket control, scenes, parameter automation, undo/redo,
  and inspectable latency/CPU/xrun telemetry
- An open node-provider boundary so built-in DSP and future providers compose
  without inheritance or a single ever-growing engine class

## Architecture consequences

- The persisted graph is an editable description; the audio thread sees only
  an immutable compiled execution plan.
- Immutable plans store exact-length boxed tables, reject render formats and
  plans above configured bounds, and never grow after installation.
- Node type identifiers and error codes are stable constant values. Display
  names are localized separately and never become protocol identifiers.
- Device limits, sample rates, buffer sizes, available applications, plug-ins,
  and permissions are resolved at runtime through capability providers.
- Virtual endpoints and application capture are separate capabilities.
  Applications may explicitly render into a ToneNari endpoint without granting
  ToneNari process-capture access; process capture uses an independent backend
  and permission path.
- On macOS 14.2 and later, application capture uses Core Audio process taps and
  a private aggregate device. ScreenCaptureKit is not the primary audio-engine
  backend because it couples capture to the screen-recording permission model.
- The shared Rust contracts do not require a kernel driver. macOS uses a Core
  Audio AudioServerPlugIn, Windows uses its signed virtual-audio driver model,
  and Linux uses PipeWire with an optional ALSA compatibility path.
- Source, processor, analyzer, and sink are roles, not inheritance hierarchies.
  A node advertises typed ports and capabilities and is composed into the graph.
- Stateful DSP, streaming, and recording implementations remain behind small
  interfaces so their policies can be unit-tested with fake clocks, buffers,
  encoders, and transports.
- Native capture resources are owned by RAII sessions. Drop and partial-failure
  paths must stop callbacks before releasing IO objects and are unit-tested for
  exactly-once cleanup.

## References

- [Loopback manual](https://rogueamoeba.com/support/manuals/loopback/?print=true)
- [Audio Hijack manual](https://rogueamoeba.com/support/manuals/audiohijack/?print=true)
- [Rogue Amoeba built-in effects overview](https://www.rogueamoeba.com/support/knowledgebase/?product=Audio+Hijack&showArticle=AboutEffects)

## Memory measurement

On 2026-09-02, the schema-v2 stereo-stream fixture was compiled in the same
debug test process before and after replacing mutable-capacity `Vec` storage in
the installed execution plan with exact-length boxed slices:

- before: 2,531 bytes reserved by the plan;
- after: 1,787 bytes reserved by the plan;
- reduction: 744 bytes (29.4%).

The compiler now also retains the validated typed node configuration needed to
lower Gain, Mixer, host bindings, and provider nodes into a render plan. Under
the same fixture and debug test process this deliberately raises the installed
execution-plan estimate from 1,787 to 3,219 bytes: 1,432 bytes (80.1%) buys the
execution contract that the earlier port-only plan had incorrectly discarded.
The real-time `RenderPlan` then lowers that configuration to numeric buffer
offsets and scalar parameters; it does not carry the serialized configuration
through the audio callback.

The measurement includes the plan's Rust structs, encoded configuration
payload estimate, owned string capacities, port tables, edge tables, and
channel-route tables. It excludes allocator metadata and the outer shared
`Arc`, so it is a stable same-condition comparison rather than a process-RSS
claim.
