#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "ToneNari Linux audio installation only runs on Linux" >&2
  exit 1
fi

for command_name in pw-cli pw-dump wpctl systemctl systemd-analyze pkg-config; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "missing required command: $command_name" >&2
    echo "Debian/Kali: sudo apt install pipewire-bin wireplumber pkg-config liblilv-dev" >&2
    exit 1
  fi
done

if ! pkg-config --exists lilv-0; then
  echo "missing required development library: lilv-0" >&2
  echo "Debian/Kali: sudo apt install liblilv-dev" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
install_root="${XDG_DATA_HOME:-$HOME/.local/share}/tonenari"
service_root="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
engine_target="$install_root/bin/tonenari-engine-service"
service_target="$service_root/io.tonenari.audio.engine.service"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"

cargo build --manifest-path "$repo_root/Cargo.toml" -p tonenari-engine-service --release
mkdir -p "$install_root/bin" "$service_root"

if [[ -e "$engine_target" ]]; then
  mv "$engine_target" "$engine_target.backup-$timestamp"
fi
if [[ -e "$service_target" ]]; then
  cp "$service_target" "$service_target.backup-$timestamp"
fi

install -m 0755 "$repo_root/target/release/tonenari-engine-service" "$engine_target"
sed "s|@ENGINE_BINARY@|$engine_target|g" \
  "$repo_root/packaging/linux/io.tonenari.audio.engine.service.in" >"$service_target"

systemd-analyze --user verify "$service_target"
systemctl --user daemon-reload
systemctl --user enable --now io.tonenari.audio.engine.service

echo "Installed ToneNari engine at $engine_target"
echo "Started user service io.tonenari.audio.engine.service"
