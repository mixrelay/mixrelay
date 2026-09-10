#!/usr/bin/env bash
set -euo pipefail
[[ $(uname -s) == Linux && $(id -u) != 0 ]] || exit 2
engine=${1:?Expected absolute engine binary}
[[ "$engine" == /* && -x "$engine" ]] || exit 2
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
if curl --silent --max-time 2 http://127.0.0.1:19341/health >/dev/null; then
  echo "Refusing to replace an existing engine" >&2
  exit 2
fi
systemd-run --user --unit=mixrelay-lab-lifecycle --collect "$engine"
trap 'systemctl --user stop mixrelay-lab-lifecycle' EXIT
ready=false
for ((attempt = 0; attempt < 50; attempt++)); do
  if curl --silent --fail --max-time 1 http://127.0.0.1:19341/health >/dev/null; then
    ready=true
    break
  fi
  sleep 0.1
done
[[ "$ready" == true ]] || exit 1
pid=$(systemctl --user show mixrelay-lab-lifecycle --property=MainPID --value)
[[ "$pid" =~ ^[1-9][0-9]*$ ]] || exit 1
printf 'enginePid=%s\n' "$pid"
curl --silent --fail --max-time 2 http://127.0.0.1:19341/api/v1/status
printf '\n'
# Inspect only directories carrying this exact live engine PID. Never clean up
# prior runs here: retained stale artifacts must not make a failed Drop look good.
mapfile -t paths < <(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type d -name "mixrelay-endpoint-$pid-*")
(( ${#paths[@]} > 0 )) || { echo "No owned endpoint paths to verify" >&2; exit 1; }
printf 'ownedEndpointDirectories=%s\n' "${#paths[@]}"
systemctl --user stop mixrelay-lab-lifecycle
trap - EXIT
for path in "${paths[@]}"; do
  [[ ! -e "$path" ]] || { printf 'Leaked endpoint: %s\n' "$path" >&2; exit 1; }
done
echo "SIGTERM joined the worker and removed its owned endpoint FIFOs/directories"
