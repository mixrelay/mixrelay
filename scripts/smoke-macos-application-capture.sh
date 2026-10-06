#!/bin/zsh
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  print -u2 "macOS application capture smoke requires macOS"
  exit 1
fi

repo_root="${0:A:h:h}"
native_root="$repo_root/native/macos/application-capture"
endpoint_uid=""
if (( $# > 1 )); then
  print -u2 "usage: $0 [exact-owned-endpoint-uid-or-canonical-uuid]"
  exit 1
fi
if (( $# == 1 )); then
  selected="$1"
  uuid_pattern='^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
  uid_pattern='^io\.mixrelay\.audio\.endpoint\.[0-9a-f]{32}$'
  if [[ "$selected" =~ "$uuid_pattern" ]]; then
    endpoint_uid="io.mixrelay.audio.endpoint.${selected//-/}"
  elif [[ "$selected" =~ "$uid_pattern" ]]; then
    endpoint_uid="$selected"
  else
    print -u2 "explicit canonical MixRelay endpoint required; refusing default fallback"
    exit 1
  fi
  if [[ "$endpoint_uid" == "io.mixrelay.audio.endpoint.00000000000000000000000000000000" ]]; then
    print -u2 "zero endpoint identity is not owned"
    exit 1
  fi
fi
cargo build --manifest-path "$repo_root/Cargo.toml" -p mixrelay-process-tap --release
capture_archive="$repo_root/target/release/libmixrelay_process_tap.a"
smoke_build_dir="$repo_root/target/native-tests/application-capture"
mkdir -p "$smoke_build_dir"
tone_process_id=""
cleanup() {
  if [[ -n "$tone_process_id" ]]; then
    if [[ -n "$endpoint_uid" ]]; then
      { print -p -- quit } 2>/dev/null || true
      for attempt in {1..50}; do
        kill -0 "$tone_process_id" 2>/dev/null || break
        sleep 0.02
      done
    fi
    kill "$tone_process_id" 2>/dev/null || true
    wait "$tone_process_id" 2>/dev/null || true
    tone_process_id=""
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
tone_name="MRToneSource"
tone_frameworks=(-framework AVFAudio -framework Foundation)
if [[ -n "$endpoint_uid" ]]; then
  tone_name="MRVirtualToneSource"
  tone_frameworks+=(-framework CoreAudio -framework AudioUnit)
fi
tone_bundle="$smoke_build_dir/$tone_name.app"
mkdir -p "$tone_bundle/Contents/MacOS"
cp "$native_root/tests/$tone_name-Info.plist" "$tone_bundle/Contents/Info.plist"

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
  "$native_root/tests/$tone_name.m" \
  "${tone_frameworks[@]}" \
  -o "$tone_bundle/Contents/MacOS/$tone_name"

clang \
  -std=c17 \
  -fobjc-arc \
  -fmodules \
  "${strict_flags[@]}" \
  -I "$native_root/include" \
  "$native_root/tests/MRProcessTapSmoke.m" \
  "$capture_archive" -liconv \
  -framework CoreAudio \
  -framework Foundation \
  -framework AppKit \
  -o "$smoke_build_dir/MRProcessTapSmoke"

if [[ -n "$endpoint_uid" ]]; then
  coproc "$tone_bundle/Contents/MacOS/$tone_name" --mixrelay-virtual-only "$endpoint_uid"
  tone_process_id=$!
  readiness=""
  if ! read -r -p -t 5 readiness || [[ "$readiness" != "ready $endpoint_uid" ]]; then
    print -u2 "owned tone did not acknowledge the exact endpoint"
    exit 1
  fi
else
  "$tone_bundle/Contents/MacOS/$tone_name" &
  tone_process_id=$!
  sleep 1
fi
set +e
if [[ -n "$endpoint_uid" ]]; then
  "$smoke_build_dir/MRProcessTapSmoke" io.mixrelay.test-virtual-clock-source "$endpoint_uid"
else
  "$smoke_build_dir/MRProcessTapSmoke" io.mixrelay.test-tone-source
fi
smoke_status=$?
set -e
if [[ -z "$endpoint_uid" ]]; then
  set +e
  wait "$tone_process_id"
  tone_status=$?
  set -e
  tone_process_id=""
  if (( tone_status != 0 )); then
    exit "$tone_status"
  fi
fi
exit "$smoke_status"
