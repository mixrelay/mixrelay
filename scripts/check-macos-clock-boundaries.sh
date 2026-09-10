#!/usr/bin/env bash
# Focused, deterministic checks. Does not open devices, install the HAL, launch
# VMs, mutate the running graph, or request microphone/system-capture permission.
set -euo pipefail
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'CoreAudio clock boundary checks require macOS.' >&2
  exit 2
fi
task_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$task_root"
cargo test -p tonenari-engine-service --bin tonenari-engine-service native_callback_and_hal_contracts -- --nocapture
# Full Rust owners replace the previous private-C device and tap fixtures.
cargo test -p tonenari-macos-transport --lib device_io:: -- --nocapture
cargo test -p tonenari-process-tap --lib -- --nocapture
cargo test -p tonenari-engine-service --bin tonenari-engine-service macos_devices::tests -- --nocapture
cargo test -p tonenari-desktop macos_audio::clock_pump::tests -- --nocapture
