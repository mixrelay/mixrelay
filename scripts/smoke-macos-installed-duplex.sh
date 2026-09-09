#!/bin/zsh
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  print -u2 "ToneNari installed duplex smoke only runs on macOS"
  exit 1
fi

repo_root="${0:A:h:h}"
mkdir -p "$repo_root/target/native-tests"
smoke_root="$(mktemp -d "$repo_root/target/native-tests/installed-duplex.XXXXXX")"
smoke_binary="$smoke_root/TNInstalledDuplexSmoke"
service_target="system/io.tonenari.audio.engine"
service_stdout="$HOME/Library/Logs/ToneNari/engine.stdout.log"
service_stderr="$HOME/Library/Logs/ToneNari/engine.stderr.log"

cleanup() {
  rm -rf -- "$smoke_root"
}
trap cleanup EXIT

clang \
  -std=c17 \
  -mmacosx-version-min=14.2 \
  -Wall \
  -Wextra \
  -Wpedantic \
  -Wconversion \
  -Wsign-conversion \
  -Wshadow \
  -Werror \
  -I "$repo_root/native/macos/hal-driver/include" \
  "$repo_root/native/macos/hal-driver/tests/TNInstalledDuplexSmoke.c" \
  -framework CoreAudio \
  -framework CoreFoundation \
  -o "$smoke_binary"

for attempt in {1..20}; do
  set +e
  smoke_output="$($smoke_binary "$@" 2>&1)"
  smoke_status=$?
  set -e
  if (( smoke_status == 0 )); then
    print -r -- "$smoke_output"
    exit 0
  fi
  if (( smoke_status != 2 )); then
    print -u2 -r -- "$smoke_output"
    break
  fi
  sleep 1
done

print -u2 "ToneNari installed duplex smoke failed"
/bin/launchctl print "$service_target" 2>&1 | sed -n '1,160p' >&2 || true
if [[ -f "$service_stdout" ]]; then
  sed -n '1,160p' "$service_stdout" >&2
fi
if [[ -f "$service_stderr" ]]; then
  sed -n '1,160p' "$service_stderr" >&2
fi
exit "${smoke_status:-1}"
