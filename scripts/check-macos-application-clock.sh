#!/bin/zsh
set -euo pipefail

# Opt-in fixture only. Exact ToneNari virtual UID; no physical/default routing,
# installed graph mutation, permission reset, or user-application capture.
repo_root="${0:A:h:h}"
fixture_root="${1:?provide a dedicated output directory}"
mkdir -p "$fixture_root/TNVirtualToneSource.app/Contents/MacOS"
native_root="$repo_root/native/macos/application-capture"
cp "$native_root/tests/TNVirtualToneSource-Info.plist" "$fixture_root/TNVirtualToneSource.app/Contents/Info.plist"
clang -fobjc-arc -fmodules -fblocks -mmacosx-version-min=14.2 \
  -Wall -Wextra -Wpedantic -Wconversion -Wsign-conversion -Wshadow -Werror \
  "$native_root/tests/TNVirtualToneSource.m" \
  -framework AVFAudio -framework Foundation -framework CoreAudio -framework AudioUnit \
  -o "$fixture_root/TNVirtualToneSource.app/Contents/MacOS/TNVirtualToneSource"
cd "$repo_root"
TONENARI_APPLICATION_CLOCK_FIXTURE=1 cargo test -p tonenari-desktop \
  live_owned_application_tap_v2_src_rate_changes --no-run
if [[ "${2:-}" == "--run" ]]; then
  TONENARI_APPLICATION_CLOCK_FIXTURE=1 \
  TONENARI_VIRTUAL_TONE_FIXTURE="$fixture_root/TNVirtualToneSource.app/Contents/MacOS/TNVirtualToneSource" \
    cargo test -p tonenari-desktop live_owned_application_tap_v2_src_rate_changes -- --ignored --nocapture
fi
print "Fixture artifacts retained: $fixture_root"
