#!/bin/sh
set -eu

for executable in /proc/[0-9]*/exe; do
  target=$(readlink "$executable" 2>/dev/null) || continue
  case "$target" in
    /usr/bin/mixrelay-desktop|'/usr/bin/mixrelay-desktop (deleted)'|\
    /usr/bin/mixrelay-engine-service|'/usr/bin/mixrelay-engine-service (deleted)'|\
    /usr/bin/mixrelay-driver-manager|'/usr/bin/mixrelay-driver-manager (deleted)')
      printf '%s\n' 'Quit MixRelay and MixRelay Driver before updating or removing the Driver.' >&2
      exit 1 ;;
  esac
done

# Resolve only actual installed Driver processes. Audio remains in their user session.
for executable in /proc/[0-9]*/exe; do
  target=$(readlink "$executable" 2>/dev/null) || continue
  case "$target" in
    /usr/bin/mixrelay-driver-service|'/usr/bin/mixrelay-driver-service (deleted)') ;;
    *) continue ;;
  esac
  process=${executable%/exe}
  uid=$(stat -c %u "$process")
  [ "$uid" -ne 0 ] || { printf '%s\n' 'Driver is incorrectly running as root.' >&2; exit 1; }
  account=$(getent passwd "$uid")
  user=${account%%:*}
  [ -n "$user" ] || exit 1
  runtime="/run/user/$uid"
  [ "$(stat -c %u "$runtime")" = "$uid" ] && [ ! -L "$runtime" ] || exit 1
  runuser -u "$user" -- env XDG_RUNTIME_DIR="$runtime" DBUS_SESSION_BUS_ADDRESS="unix:path=$runtime/bus" \
    systemctl --user stop mixrelay-driver.service
  attempts=0
  while [ -e "$process/exe" ]; do
    attempts=$((attempts + 1))
    [ "$attempts" -lt 50 ] || { printf '%s\n' 'Driver did not drain after service stop.' >&2; exit 1; }
    sleep 0.1
  done
done
