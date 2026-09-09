# Linux packaging

ToneNari runs as a per-user PipeWire client. Debian and Kali use the same backend and require
`pipewire-bin`, `wireplumber`, `pkg-config`, and `liblilv-dev`; a PulseAudio-only session is
intentionally rejected rather than reported as ready. Lilv provides LV2 discovery and hosting,
while VST3 and CLAP use their standard user/global install paths without a registration service.

The engine loads `libpipewire-module-pipe-tunnel` modules for the graph-owned virtual sink/source
and per-application capture taps. Module ownership stays with one long-lived `pw-cli` process and
is released when the graph changes or the service stops. FIFO paths live below `XDG_RUNTIME_DIR`,
are mode `0600`, and are removed by deterministic `Drop` paths.

For a development install run `pnpm install:linux-dev-audio`. The script builds the release engine,
backs up an existing installed binary/service file, installs a user systemd service, and starts it.
