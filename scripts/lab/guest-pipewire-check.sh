#!/usr/bin/env bash
set -euo pipefail
[[ $(uname -s) == Linux && $(id -u) != 0 ]] || exit 2
engine=${1:?Expected engine binary}
node=${2:?Expected Node binary}
probe=${3:?Expected probe script}
[[ "$engine" == /* && -x "$engine" && "$node" == /* && -x "$node" ]] || exit 2
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"

# Never replace a running user engine or reuse its graph for a synthetic test.
if curl --silent --max-time 2 http://127.0.0.1:19341/health >/dev/null; then
  echo "Refusing to test over an existing engine" >&2
  exit 2
fi
uname -a
pw-cat --version
sha256sum "$engine" "$probe"
result=0
for run in 1 2 3; do
  printf '\nProbe run %s\n' "$run"
  # Each measurement owns fresh FIFOs. Reusing an engine would include queued
  # audio from the previous player in the next recording's continuity window.
  systemd-run --user --unit=tonenari-lab-engine --collect "$engine"
  trap 'systemctl --user stop tonenari-lab-engine' EXIT
  ready=false
  for ((attempt = 0; attempt < 50; attempt++)); do
    if curl --silent --fail --max-time 1 http://127.0.0.1:19341/health >/dev/null; then
      ready=true
      break
    fi
    sleep 0.1
  done
  [[ "$ready" == true ]] || { echo "Engine did not become ready" >&2; exit 1; }
  if "$node" "$probe" --run; then
    echo "Probe exit: 0"
  else
    code=$?
    printf 'Probe exit: %s\n' "$code"
    result=1
  fi
  systemctl --user stop tonenari-lab-engine
  trap - EXIT
done
exit "$result"
