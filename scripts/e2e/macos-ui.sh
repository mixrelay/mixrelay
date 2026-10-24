#!/usr/bin/env bash
set -euo pipefail
output="$(cd "$1" && pwd)"
repository="$(pwd)"
tools="$output/ui-tools"
mkdir "$tools"
cp scripts/e2e/macos-ui/project.yml scripts/e2e/macos-ui/*.swift "$tools/"
export MIXRELAY_UI_EVIDENCE="$output"
cd "$tools"
xcodegen generate --spec project.yml
node "$repository/scripts/e2e/macos-ui-runner.mjs" "$output" "$tools"
