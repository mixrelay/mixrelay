#!/bin/zsh
# Measures latency, bit exactness, channel order, slips and dropouts of an audio path on
# installed devices (any vendor). See native/macos/hal-driver/tests/MRRoundTripProbe.c.
#   pnpm probe:macos-roundtrip --transport DEVICE_UID
#   pnpm probe:macos-roundtrip --list
#   pnpm probe:macos-roundtrip [--json] [--seconds S] [--channels N] OUTPUT_UID [INPUT_UID]
#   pnpm probe:macos-roundtrip --self-test
# Recording needs microphone permission for the terminal that runs it; without it every
# input reads as silence and the probe exits with status 3.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  print -u2 "the round-trip probe only runs on macOS"
  exit 1
fi
repo_root="${0:A:h:h}"
source_file="$repo_root/native/macos/hal-driver/tests/MRRoundTripProbe.c"
binary="$repo_root/target/macos-probes/MRRoundTripProbe"
if [[ ! -x "$binary" || "$source_file" -nt "$binary" ]]; then
  mkdir -p "${binary:h}"
  clang -std=c17 -O2 -mmacosx-version-min=14.2 \
    -Wall -Wextra -Wpedantic -Wconversion -Wsign-conversion -Wshadow -Werror \
    "$source_file" -framework CoreAudio -framework CoreFoundation -o "$binary"
fi
exec "$binary" "$@"
