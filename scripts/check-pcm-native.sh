#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$repo/target/native-tests"
test_dir=$(mktemp -d "$repo/target/native-tests/pcm.XXXXXX")
# This directory is created exclusively above and contains only this test's binary.
trap 'rm -f -- "$test_dir/pcm-tests"; rmdir -- "$test_dir"' EXIT
"${CC:-clang}" -std=c17 -Wall -Wextra -Werror -pthread \
  -fsanitize=address,undefined -fno-omit-frame-pointer \
  -I "$repo/native/linux/pipewire" \
  "$repo/native/linux/pipewire/TNPcmRing.c" \
  "$repo/native/linux/pipewire/tests/TNPcmRingTests.c" \
  -o "$test_dir/pcm-tests"
"$test_dir/pcm-tests"
