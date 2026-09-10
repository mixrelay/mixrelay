#!/bin/zsh
set -euo pipefail

# Opt-in fixture only. Exact MixRelay virtual UID; no physical/default routing,
# installed graph mutation, permission reset, or user-application capture.
repo_root="${0:A:h:h}"
fixture_root="${1:?provide a dedicated output directory}"
mkdir -p "$fixture_root/MRVirtualToneSource.app/Contents/MacOS"
native_root="$repo_root/native/macos/application-capture"
cp "$native_root/tests/MRVirtualToneSource-Info.plist" "$fixture_root/MRVirtualToneSource.app/Contents/Info.plist"
clang -fobjc-arc -fmodules -fblocks -mmacosx-version-min=14.2 \
  -Wall -Wextra -Wpedantic -Wconversion -Wsign-conversion -Wshadow -Werror \
  "$native_root/tests/MRVirtualToneSource.m" \
  -framework AVFAudio -framework Foundation -framework CoreAudio -framework AudioUnit \
  -o "$fixture_root/MRVirtualToneSource.app/Contents/MacOS/MRVirtualToneSource"
cd "$repo_root"
MIXRELAY_APPLICATION_CLOCK_FIXTURE=1 cargo test -p mixrelay-desktop \
  live_owned_application_tap_v2_src_rate_changes --no-run
if [[ "${2:-}" == "--run" ]]; then
  MIXRELAY_APPLICATION_CLOCK_FIXTURE=1 \
  MIXRELAY_VIRTUAL_TONE_FIXTURE="$fixture_root/MRVirtualToneSource.app/Contents/MacOS/MRVirtualToneSource" \
    cargo test -p mixrelay-desktop live_owned_application_tap_v2_src_rate_changes -- --ignored --nocapture
fi
print "Fixture artifacts retained: $fixture_root"
