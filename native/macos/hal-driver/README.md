# ToneNari macOS HAL driver

This directory is the isolated build boundary for the native
`AudioServerPlugIn`. Implementation starts from Apple's MIT-licensed
NullAudio sample and must not copy BlackHole GPL source.

Non-negotiable implementation constraints:

- no allocation, locking, logging, or blocking work in real-time callbacks;
- configuration changes go through `RequestDeviceConfigurationChange`;
- integer host-time accounting and an incrementing timeline seed;
- unique object and device identifiers with no shared mutable device state;
- generation-tagged ring-buffer validity instead of clearing the full buffer;
- stable published UIDs across versions and locales;
- dedicated tests for wraparound, large buffers, restart, sleep, and drift.

The production topology is two buses: `ToneNari Capture` and
`ToneNari Stream`.
