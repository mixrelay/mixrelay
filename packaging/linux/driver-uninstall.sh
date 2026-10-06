#!/bin/sh

{
set -eu
[ "$(id -u)" -ne 0 ] || { printf '%s\n' 'Run Driver removal as the signed-in user.' >&2; exit 1; }
case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  ko*) title='MixRelay Driver 제거'; message='가상 장치와 Driver 서비스를 제거합니다. 앱 그래프·녹음·유료 인증은 보존합니다.'; delete_label='저장된 Driver 장치 설정 삭제'; failed='Driver를 제거하지 못했습니다. 앱과 Driver 관리자를 종료한 뒤 다시 시도하세요.'; auth_cancelled='인증이 취소되어 Driver 패키지를 제거하지 않았습니다. 저장된 설정은 삭제하지 않았습니다.'; auth_failed='승인 불가 또는 인증 오류로 Driver 패키지를 제거하지 않았습니다. 저장된 설정은 삭제하지 않았습니다.' ;;
  *) title='Uninstall MixRelay Driver'; message='Remove virtual devices and the Driver service. App graphs, recordings and paid authentication remain.'; delete_label='Delete saved Driver device settings'; failed='Driver removal failed. Quit MixRelay and the Driver Manager, then retry.'; auth_cancelled='Authentication cancelled. The Driver package was not removed and saved settings were not deleted.'; auth_failed='Authorization unavailable or authentication failed. The Driver package was not removed and saved settings were not deleted.' ;;
esac
remove_package() {
  if pkexec "$@"; then return 0; else status=$?; fi
  case "$status" in
    126) zenity --info --title="$title" --text="$auth_cancelled" ;;
    127) zenity --error --title="$title" --text="$auth_failed" ;;
    *) zenity --error --title="$title" --text="$failed"; exit 1 ;;
  esac
  exit "$status"
}
home=${HOME:?}
case "$home" in /*) ;; *) exit 1 ;; esac
case "${XDG_CONFIG_HOME:-}" in /*) config=$XDG_CONFIG_HOME ;; *) config="$home/.config" ;; esac
settings="$config/mixrelay-driver/drivers.json"
# Validate only this exact settings path before asking for authorization.
current=/
remaining=${settings#/}
while [ -n "$remaining" ]; do
  component=${remaining%%/*}
  case "$component" in ''|.|..) exit 1 ;; esac
  current="${current%/}/$component"
  [ ! -L "$current" ] || exit 1
  if [ "$remaining" = "$component" ]; then remaining=; else remaining=${remaining#*/}; fi
done
selection=$(zenity --list --checklist --title="$title" --text="$message" \
  --column='' --column='' FALSE "$delete_label") || exit 0
if command -v dpkg-query >/dev/null 2>&1 && dpkg-query -W -f='${db:Status-Status}' mix-relay-driver 2>/dev/null | grep -Eq '^(installed|unpacked|half-configured|half-installed|triggers-awaited|triggers-pending)$'; then
  remove_package /usr/bin/apt-get -y remove -- mix-relay-driver
elif command -v rpm >/dev/null 2>&1 && rpm -q mix-relay-driver >/dev/null 2>&1; then
  remove_package /usr/bin/dnf -y --setopt=clean_requirements_on_remove=False remove mix-relay-driver
else
  zenity --error --title="$title" --text="$failed"
  exit 1
fi
if [ "$selection" = "$delete_label" ]; then
  # User privileges and a fixed regular filename: never recursively remove a directory.
  [ ! -d "$settings" ] && [ ! -L "$settings" ] || exit 1
  rm -f -- "$settings"
fi
}
