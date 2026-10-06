#!/bin/sh
set -eu
[ "$(id -u)" -ne 0 ] || { printf '%s\n' 'Run MixRelay Driver as the signed-in user.' >&2; exit 1; }
systemctl --user start mixrelay-driver.service
case "${1:-}" in
  '') ;;
  --manager) exec /usr/bin/mixrelay-driver-manager ;;
  *) exit 2 ;;
esac
