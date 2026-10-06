#!/bin/sh
set -eu

# RPM removes the old package after installing the replacement. Its %pre
# already checked the old processes; %preun must not reject the new app.
if [ "$1" -gt 0 ]; then
  exit 0
fi

for executable in /proc/[0-9]*/exe; do
  target=$(readlink "$executable" 2>/dev/null) || continue
  case "$target" in
    /usr/bin/mixrelay-desktop|'/usr/bin/mixrelay-desktop (deleted)'|\
    /usr/bin/mixrelay-engine-service|'/usr/bin/mixrelay-engine-service (deleted)')
      printf '%s\n' 'Quit MixRelay from its menu or tray, then retry the removal.' >&2
      exit 1
      ;;
  esac
done

exit 0
