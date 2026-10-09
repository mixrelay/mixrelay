#!/bin/sh
set -eu

case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  en*) instruction='Quit MixRelay and MixRelay Driver before updating or removing MixRelay Driver.' ;;
  ko*) instruction='MixRelay Driver를 업데이트하거나 제거하기 전에 MixRelay와 MixRelay Driver를 종료하세요.' ;;
  ja*) instruction='MixRelay Driverの更新または削除前にMixRelayとMixRelay Driverを終了してください。' ;;
  zh_Hans*|zh-Hans*) instruction='更新或移除 MixRelay Driver 前，请退出 MixRelay 和 MixRelay Driver。' ;;
  zh_Hant*|zh-Hant*|zh_TW*|zh-TW*|zh_HK*|zh-HK*|zh_MO*|zh-MO*) instruction='更新或移除 MixRelay Driver 前，請結束 MixRelay 與 MixRelay Driver。' ;;
  zh*) instruction='更新或移除 MixRelay Driver 前，请退出 MixRelay 和 MixRelay Driver。' ;;
  fr*) instruction='Quittez MixRelay et MixRelay Driver avant de mettre à jour ou de supprimer MixRelay Driver.' ;;
  de*) instruction='Beenden Sie MixRelay und MixRelay Driver vor dem Aktualisieren oder Entfernen von MixRelay Driver.' ;;
  es*) instruction='Cierre MixRelay y MixRelay Driver antes de actualizar o eliminar MixRelay Driver.' ;;
  pt*) instruction='Feche o MixRelay e o MixRelay Driver antes de atualizar ou remover o MixRelay Driver.' ;;
  it*) instruction='Chiudere MixRelay e MixRelay Driver prima di aggiornare o rimuovere MixRelay Driver.' ;;
  ru*) instruction='Перед обновлением или удалением MixRelay Driver закройте MixRelay и MixRelay Driver.' ;;
  *) instruction='Quit MixRelay and MixRelay Driver before updating or removing MixRelay Driver.' ;;
esac

for executable in /proc/[0-9]*/exe; do
  target=$(readlink "$executable" 2>/dev/null) || continue
  case "$target" in
    /usr/bin/mixrelay-desktop|'/usr/bin/mixrelay-desktop (deleted)'|\
    /usr/bin/mixrelay-engine-service|'/usr/bin/mixrelay-engine-service (deleted)'|\
    /usr/bin/mixrelay-driver-manager|'/usr/bin/mixrelay-driver-manager (deleted)')
      printf '%s\n' "$instruction" >&2
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
  [ "$uid" -ne 0 ] || { printf '%s\n' 'MixRelay Driver is incorrectly running as root.' >&2; exit 1; }
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
    [ "$attempts" -lt 50 ] || { printf '%s\n' 'MixRelay Driver did not drain after service stop.' >&2; exit 1; }
    sleep 0.1
  done
done
