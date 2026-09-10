# ADR 0001: Single repository with explicit build boundaries

Status: Accepted

MixRelay uses one private Git repository containing the React control surface,
Tauri shell, Rust services, native macOS HAL driver, packaging, and integration
tests.

The repository is unified, but the real-time driver is not absorbed into the
Rust or JavaScript build. Cargo and pnpm workspaces provide local dependency
management while the native driver retains an independent Apple toolchain and
signing boundary.

The responsive control UI is shared by the desktop webview and authenticated
LAN browser clients. Transport-specific behavior is injected behind a
`ControlPort` interface.
