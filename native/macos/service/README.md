# MixRelay macOS engine service native boundary

This directory provides the C ABI headers and the Blocks-based XPC listener
adapter for the Rust graph engine. The launchd Mach service
`io.mixrelay.audio.engine` handles application-source transport attachment and
engine wake messages. Rust owns listener and peer lifetimes, request handling,
and the graph-owned application-source registry; `src/MRService.c`
bridges XPC event-handler Blocks into that Rust implementation.

Application capture producers obtain a descriptor for a separate shared-memory
source transport through this listener. Captured PCM travels through that
transport to the graph, not through XPC messages.

Virtual-device control belongs to the Driver service
(`io.mixrelay.audio.driver`). The HAL connects to that service for endpoint
inventory and transport descriptors. The Driver service allocates anonymous
shared memory and owns mapping replacement and inventory generation policy.
The endpoint rings hold HAL-owned loopback audio: HAL publishes playback into
its capture storage without requiring the graph engine to run.

The graph engine opens MixRelay virtual devices and other audio devices as
ordinary CoreAudio clients. Device IOProc callbacks supply the graph's device
I/O; the engine does not directly process the HAL endpoint rings. The headers
and fixtures here also describe the native device, aggregate, and application
clock boundaries implemented by the Rust audio adapters.

The current engine listener accepts peers belonging to the console user. This
is a development baseline; signed production peer-identity validation remains
required before exposing live application-source transport.
