#!/usr/bin/env bash
set -euo pipefail
output="$(cd "$1" && pwd)"
tools="$output/ui-tools"
mkdir "$tools"
cp scripts/e2e/macos-ui/project.yml scripts/e2e/macos-ui/*.swift "$tools/"
export MIXRELAY_UI_EVIDENCE="$output"
cd "$tools"
xcodegen generate --spec project.yml
xcodebuild test -project MixRelayInstalledUI.xcodeproj -scheme InstalledUI \
  -destination 'platform=macOS' -derivedDataPath DerivedData \
  -resultBundlePath "$output/installed-ui.xcresult" CODE_SIGNING_ALLOWED=NO
