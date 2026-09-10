#!/bin/zsh
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  print -u2 "macOS application capture smoke requires macOS"
  exit 1
fi

repo_root="${0:A:h:h}"
native_root="$repo_root/native/macos/application-capture"
cargo build --manifest-path "$repo_root/Cargo.toml" -p tonenari-process-tap --release
capture_archive="$repo_root/target/release/libtonenari_process_tap.a"
mkdir -p "$repo_root/target/native-tests"
smoke_build_dir="$(mktemp -d "$repo_root/target/native-tests/application-capture.XXXXXX")"
trap 'rm -rf -- "$smoke_build_dir"' EXIT
tone_bundle="$smoke_build_dir/TNToneSource.app"
mkdir -p "$tone_bundle/Contents/MacOS"
cp "$native_root/tests/TNToneSource-Info.plist" "$tone_bundle/Contents/Info.plist"

strict_flags=(
  -mmacosx-version-min=14.2
  -Wall
  -Wextra
  -Wpedantic
  -Wconversion
  -Wsign-conversion
  -Wshadow
  -Werror
)

clang \
  -fobjc-arc \
  -fmodules \
  -fblocks \
  "${strict_flags[@]}" \
  "$native_root/tests/TNToneSource.m" \
  -framework AVFAudio \
  -framework Foundation \
  -o "$tone_bundle/Contents/MacOS/TNToneSource"

clang \
  -std=c17 \
  -fobjc-arc \
  -fmodules \
  "${strict_flags[@]}" \
  -I "$native_root/include" \
  "$native_root/tests/TNProcessTapSmoke.m" \
  "$capture_archive" -liconv \
  -framework CoreAudio \
  -framework Foundation \
  -framework AppKit \
  -o "$smoke_build_dir/TNProcessTapSmoke"

"$tone_bundle/Contents/MacOS/TNToneSource" &
tone_process_id=$!
sleep 1
set +e
"$smoke_build_dir/TNProcessTapSmoke" io.tonenari.test-tone-source
smoke_status=$?
set -e
wait "$tone_process_id"
exit "$smoke_status"
