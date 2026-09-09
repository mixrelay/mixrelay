#!/usr/bin/env bash
# Focused, deterministic checks. Does not open devices, install the HAL, launch
# VMs, mutate the running graph, or request microphone/system-capture permission.
set -euo pipefail
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'CoreAudio clock boundary checks require macOS.' >&2
  exit 2
fi
task_root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$task_root/target/native-tests"
task_build="$(mktemp -d "$task_root/target/native-tests/clock-boundaries.XXXXXX")"
trap 'printf "Clock boundary artifacts: %s\n" "$task_build"' EXIT
cd "$task_root"
cargo test -p tonenari-engine-service --bin tonenari-engine-service native_callback_and_hal_contracts -- --nocapture
clang -fobjc-arc -fblocks -fsanitize=address,undefined -g \
  -mmacosx-version-min=14.2 -Wall -Wextra -Werror \
  -I native/macos/application-capture/include \
  native/macos/application-capture/tests/TNProcessTapLifetimeTests.m \
  native/macos/application-capture/src/TNAudioRing.c \
  -framework Foundation -framework AppKit -framework CoreAudio \
  -o "$task_build/tap-lifetime"
"$task_build/tap-lifetime"
cargo test -p tonenari-engine-service --bin tonenari-engine-service macos_devices::tests -- --nocapture
cargo test -p tonenari-desktop macos_audio::clock_pump::tests -- --nocapture
