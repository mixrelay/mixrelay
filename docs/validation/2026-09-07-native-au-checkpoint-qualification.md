# macOS Apple AU isolated-worker state qualification

The opt-in `apps/engine-service/examples/native_au_checkpoint.rs` harness exercises
the actual Apple `AUPeakLimiter` through ToneNari's scanner, probe, isolated worker,
shared state mailbox, engine controller, and durable graph catalog. It loads only
`aufx:lmtr:appl`; all other formats return empty fixture inventories and any attempt
to probe/load a different identity is rejected.

Run explicitly:

```sh
cargo run -p tonenari-engine-service --example native_au_checkpoint -- --run
cargo clippy -p tonenari-engine-service --example native_au_checkpoint -- -D warnings
cargo test -p tonenari-processor-host --lib apple_limiter_native_state_restores_a_changed_parameter -- --nocapture
```

The parent creates a new temporary directory and supplies `TONENARI_CONFIG_DIR` only
to its child. There is no HTTP listener, HAL access, real/virtual audio endpoint,
default-device change, third-party installation, or graphical editor opening. The
temporary catalog remains available at the path printed by the harness for inspection.

## Observed passing assertions

- Apple AUPeakLimiter parameter ID `0` receives normalized `0.37` through the actual
  worker mailbox. The engine's close command checkpoints native ClassInfo state and
  removes the stale JSON parameter overlay. Encoded native state is **236 bytes**.
- Switching to a different empty graph and back creates a new isolated AU worker.
  Re-exported native ClassInfo bytes exactly match the saved state.
- Changing the actual native parameter to `0.91` produces different native state.
- An explicit graph-application Undo delta to `0.37` restores the exact earlier
  native state after worker replacement.
- The editor-state monitor's graceful finish saves state, the on-disk catalog contains
  the same native state, and destruction followed by a fresh controller/worker restores
  and re-exports that state exactly.
- The separate native AU unit test reads `plugin.parameter_value` before/after native
  ClassInfo restoration and passes with tolerance `0.001`; this is not a mock JSON test.
- Example-specific Clippy passes. No qualification worker processes remained after exit.

## Limits

Apple's in-box limiter reports `hasEditor=false` because its generic factory is not a
vendor editor. The test exercises the explicit close/checkpoint API but **does not**
qualify a real native-window X-close, mouse-driven vendor editor changes, or graphical
editor resizing/focus. The control changes are delivered through the real plug-in
parameter API. Those GUI behaviors require a suitable installed vendor fixture and
separate explicit qualification. This test also makes no Windows/Linux, crash/power-loss,
DSP latency, or multichannel audio-throughput claim.
