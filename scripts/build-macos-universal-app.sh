#!/bin/zsh
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  print -u2 "ToneNari's Universal app can only be built on macOS"
  exit 1
fi

repo_root="${0:A:h:h}"

if [[ -z "${DEVELOPER_DIR:-}" ]]; then
  selected_developer_dir="$(xcode-select -p 2>/dev/null || true)"
  if [[ "$selected_developer_dir" == *".app/Contents/Developer" ]]; then
    export DEVELOPER_DIR="$selected_developer_dir"
  elif [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
    export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
  elif [[ -d /Applications/Xcode-26.6.0.app/Contents/Developer ]]; then
    export DEVELOPER_DIR=/Applications/Xcode-26.6.0.app/Contents/Developer
  else
    print -u2 "A full Xcode installation is required; set DEVELOPER_DIR explicitly"
    exit 1
  fi
fi

installed_targets="$(rustup target list --installed)"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
  if [[ $'\n'"$installed_targets"$'\n' != *$'\n'"$target"$'\n'* ]]; then
    print -u2 "Missing Rust target $target; install it with: rustup target add $target"
    exit 1
  fi
done

export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export MACOSX_DEPLOYMENT_TARGET=14.2
compiler="$(xcrun --sdk macosx --find clang)"
export CC_aarch64_apple_darwin="$compiler"
export CC_x86_64_apple_darwin="$compiler"
export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="$compiler"
export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="$compiler"

cd "$repo_root"
pnpm --filter @tonenari/desktop tauri build \
  --bundles app \
  --target universal-apple-darwin

app_bundle="$repo_root/target/universal-apple-darwin/release/bundle/macos/ToneNari.app"
if [[ ! -d "$app_bundle" ]]; then
  print -u2 "Expected app bundle was not produced: $app_bundle"
  exit 1
fi

mach_o_count=0
while IFS= read -r -d '' candidate; do
  file_description="$(file -b "$candidate")"
  if [[ "$file_description" != *"Mach-O"* ]]; then
    continue
  fi

  architectures="$(xcrun lipo -archs "$candidate")"
  architecture_count="$(print -r -- "$architectures" | wc -w | tr -d ' ')"
  if [[ "$architecture_count" != "2" || " $architectures " != *" arm64 "* || " $architectures " != *" x86_64 "* ]]; then
    print -u2 "Every bundled Mach-O must contain exactly arm64 and x86_64"
    print -u2 "Invalid binary: $candidate ($architectures)"
    exit 1
  fi
  mach_o_count=$((mach_o_count + 1))
  print "Universal Mach-O: ${candidate#$app_bundle/} ($architectures)"
done < <(find "$app_bundle" -type f -print0)

if (( mach_o_count == 0 )); then
  print -u2 "No Mach-O executable was found in $app_bundle"
  exit 1
fi

print "ToneNari Universal app: $app_bundle"
