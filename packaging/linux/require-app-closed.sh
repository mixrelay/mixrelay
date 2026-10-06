#!/bin/sh
set -eu

# The installed desktop owns the interactive engine. Let the user save and quit
# before the package manager replaces or removes either executable.
for executable in /proc/[0-9]*/exe; do
  target=$(readlink "$executable" 2>/dev/null) || continue
  case "$target" in
    /usr/bin/mixrelay-desktop|'/usr/bin/mixrelay-desktop (deleted)'|\
    /usr/bin/mixrelay-engine-service|'/usr/bin/mixrelay-engine-service (deleted)')
      printf '%s\n' 'Quit MixRelay from its menu or tray, then retry the installation, update or removal.' >&2
      exit 1
      ;;
  esac
done

exit 0
