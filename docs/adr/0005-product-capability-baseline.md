# ADR 0005: Treat Loopback and Audio Hijack parity as the product baseline

## Status

Accepted

## Context

Rogue Amoeba splits its audio product into two adjacent models:

- Loopback configures persistent virtual devices as sources, output channels,
  and monitors. It includes Pass-Thru, application and physical-device capture,
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

### Routing and virtual devices

- Persistent, independently enabled virtual input/output devices
- Pass-Thru input with automatic one-to-one mapping
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

- Application/process audio, all system audio, Core Audio input devices,
  pass-thru/virtual inputs, and file playback

### Processing and utility nodes

- Gain/volume, balance, pan, channel operations, mono enhancement, bass/treble,
  low-pass, 10-band EQ, parametric EQ, and simple compressor
- Magic-boost style loudness processing, ducking/sidechain, mixer, input switch,
  sync/delay, and time-shift buffer
- Declick, dehum, learned denoise, speech denoise, and FIR convolution
- Peak/RMS/LUFS meters and waveform/spectrum inspection
- macOS Audio Unit hosting. Third-party processing is isolated so a plug-in
  failure cannot corrupt the graph controller.

### Outputs and operations

- Physical output and ToneNari virtual-device output
- Recording with WAV, AIFF, FLAC, ALAC, AAC, and MP3 targets
- RTMP live stream and Icecast/Shoutcast broadcast targets with metadata
- Local transcription, where the selected model is available
- Presets, reusable graph templates, import/export, global hotkeys, schedules,
  event automation, and Shortcuts-compatible control
- Background execution, menu-bar control, launch-at-login, and restart recovery

### ToneNari extensions beyond parity

- One graph model instead of requiring separate routing and processing apps
- OBS-oriented templates and named multi-track buses
- Authenticated LAN control from a phone with no cloud dependency
- Versioned HTTP/WebSocket control, scenes, parameter automation, undo/redo,
  and inspectable latency/CPU/xrun telemetry
- An open node-provider boundary so built-in DSP and future providers compose
  without inheritance or a single ever-growing engine class

## Architecture consequences

- The persisted graph is an editable description; the audio thread sees only
  an immutable compiled execution plan.
- Node type identifiers and error codes are stable constant values. Display
  names are localized separately and never become protocol identifiers.
- Device limits, sample rates, buffer sizes, available applications, plug-ins,
  and permissions are resolved at runtime through capability providers.
- Source, processor, analyzer, and sink are roles, not inheritance hierarchies.
  A node advertises typed ports and capabilities and is composed into the graph.
- Stateful DSP, streaming, and recording implementations remain behind small
  interfaces so their policies can be unit-tested with fake clocks, buffers,
  encoders, and transports.

## References

- [Loopback manual](https://rogueamoeba.com/support/manuals/loopback/?print=true)
- [Audio Hijack manual](https://rogueamoeba.com/support/manuals/audiohijack/?print=true)
- [Rogue Amoeba built-in effects overview](https://www.rogueamoeba.com/support/knowledgebase/?product=Audio+Hijack&showArticle=AboutEffects)
