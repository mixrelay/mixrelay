#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/../../../.." && pwd)
cd "$root"
cargo build --release -p mixrelay-pcm-ring
test_dir=$(mktemp -d)
trap 'rm -rf "$test_dir"' EXIT
# pkg-config emits compiler/linker argument lists for the installed PipeWire.
read -r -a pipewire_flags <<< "$(pkg-config --cflags --libs libpipewire-0.3)"
"${CC:-cc}" -std=c17 -O2 -Wall -Wextra -Werror \
    -I native/pcm-ring/include \
    native/linux/pipewire/tests/MRStreamBufferMetadataTest.c \
    "${CARGO_TARGET_DIR:-target}/release/libmixrelay_pcm_ring.a" \
    "${pipewire_flags[@]}" -lpthread -ldl -lm -o "$test_dir/buffer-metadata"
"$test_dir/buffer-metadata"
