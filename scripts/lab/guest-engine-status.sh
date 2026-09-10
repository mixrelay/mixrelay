#!/usr/bin/env bash
set -euo pipefail
[[ $(id -u) != 0 ]] || exit 2
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
systemctl --user status mixrelay-lab-engine --no-pager
journalctl --user -u mixrelay-lab-engine --no-pager -n 40
for route in health api/v1/status api/v1/graph api/v1/meters; do
  printf '\n%s\n' "$route"
  curl --fail --silent --show-error --max-time 5 "http://127.0.0.1:19341/$route"
done
wpctl status
