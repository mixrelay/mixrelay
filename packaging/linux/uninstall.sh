#!/bin/sh

# Parse the complete block before package removal can delete this script.
{
set -eu

case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  ko*)
    title='MixRelay 제거'
    message='MixRelay 메뉴나 트레이에서 종료한 뒤 제거하세요. 그래프와 설정은 기본으로 보존합니다. 녹음 파일과 내보낸 프로젝트는 보존하며, 유료 라이선스 로그인 정보는 제거합니다.'
    delete_label='저장된 그래프와 설정 삭제'
    remove_label='제거'
    cancel_label='취소'
    failed='제거하지 못했습니다. MixRelay를 종료하고 권한 요청을 승인한 뒤 다시 시도하세요. 저장된 데이터는 삭제하지 않았습니다.'
    unsafe='데이터 경로가 안전하지 않아 제거를 중단했습니다. 저장된 데이터는 삭제하지 않았습니다.'
    cleanup_failed='앱은 제거했지만 일부 그래프 또는 설정을 삭제하지 못했습니다.'
    auth_failed='앱은 제거했지만 라이선스 로그인 정보를 지우지 못했습니다. 앱을 다시 설치하고 제거를 재시도하세요. 그래프와 설정은 보존했습니다.'
    root_error='로그인한 일반 사용자로 MixRelay 제거를 실행하세요.'
    missing='설치된 MixRelay 패키지를 찾지 못했습니다.'
    done_message='MixRelay를 제거했습니다.'
    ;;
  *)
    title='Uninstall MixRelay'
    message='Quit MixRelay from its menu or tray before uninstalling. Graphs and settings are preserved by default. Recordings and exported projects are preserved. Paid-license sign-in will be cleared.'
    delete_label='Delete saved graphs and settings'
    remove_label='Uninstall'
    cancel_label='Cancel'
    failed='Uninstall failed. Quit MixRelay, approve the authorization request and retry. Saved data was not deleted.'
    unsafe='Uninstall stopped because a data path is unsafe. Saved data was not deleted.'
    cleanup_failed='The app was removed, but some saved graphs or settings could not be deleted.'
    auth_failed='The app was removed, but license sign-in could not be cleared. Reinstall the app and retry uninstalling. Graphs and settings were preserved.'
    root_error='Run Uninstall MixRelay as the signed-in regular user.'
    missing='The installed MixRelay package was not found.'
    done_message='MixRelay was uninstalled.'
    ;;
esac

show_error() {
  /usr/bin/zenity --error --title="$title" --text="$1" || :
  exit 1
}

if [ "$(/usr/bin/id -u)" = 0 ]; then
  show_error "$root_error"
fi

if [ -x /usr/bin/dpkg-query ] && [ "$(/usr/bin/dpkg-query -W -f='${db:Status-Status}' mix-relay 2>/dev/null || :)" = installed ]; then
  manager=apt
elif [ -x /usr/bin/rpm ] && /usr/bin/rpm -q mix-relay >/dev/null 2>&1; then
  manager=dnf
else
  show_error "$missing"
fi

selection=$(/usr/bin/zenity --list --checklist --title="$title" --text="$message" \
  --column='' --column="$delete_label" --hide-header --print-column=2 \
  --ok-label="$remove_label" --cancel-label="$cancel_label" FALSE "$delete_label") || exit 0

safe_path() {
  case "$1" in
    /*) ;;
    *) return 1 ;;
  esac
  case "$1" in
    *'/../'*|*'/./'*|*'//'*) return 1 ;;
  esac
  checked_path=$1
  while [ "$checked_path" != / ]; do
    [ ! -L "$checked_path" ] || return 1
    checked_path=${checked_path%/*}
    [ -n "$checked_path" ] || checked_path=/
  done
}

case "${HOME-}" in
  ''|/|/home|/root) show_error "$unsafe" ;;
  /*) ;;
  *) show_error "$unsafe" ;;
esac
case "${XDG_DATA_HOME-}" in
  /) show_error "$unsafe" ;;
  /*) data_base=$XDG_DATA_HOME ;;
  *) data_base=$HOME/.local/share ;;
esac
data=$data_base/io.mixrelay.desktop
safe_path "$data/license-remote-change.json" || show_error "$unsafe"
safe_path "$data/license-remote-change-dev.json" || show_error "$unsafe"

if [ "$selection" = "$delete_label" ]; then
  case "${XDG_CONFIG_HOME-}" in
    /) show_error "$unsafe" ;;
    /*) config_base=$XDG_CONFIG_HOME ;;
    *) config_base=$HOME/.config ;;
  esac
  config=$config_base/mixrelay
  # Only current, fixed application-owned names are eligible; external saved
  # files and the credential record's trial/clock protection remain intact.
  set -- "$config/graphs.json" "$config/audio-session.json" "$config/graphs.states" \
    "$data/recording-settings.json" "$data/WebKitCache" "$data/MediaCache" \
    "$data/applications" "$data/CacheStorage"
  for path do
    safe_path "$path" || show_error "$unsafe"
  done
fi

# Keep the installed ELF open in this user's shell. Linux proc exposes the
# retained inode after unlink, so no copied executable or elevated user is needed.
[ -r /usr/bin/mixrelay-desktop ] || show_error "$failed"
exec 3</usr/bin/mixrelay-desktop
case "$manager" in
  apt) /usr/bin/pkexec --disable-internal-agent /usr/bin/apt -y remove mix-relay || show_error "$failed" ;;
  dnf) /usr/bin/pkexec --disable-internal-agent /usr/bin/dnf --setopt=clean_requirements_on_remove=False -y remove mix-relay || show_error "$failed" ;;
esac

if ! /proc/self/fd/3 --clear-local-license; then
  exec 3<&-
  show_error "$auth_failed"
fi
exec 3<&-
safe_path "$data/license-remote-change.json" || show_error "$auth_failed"
safe_path "$data/license-remote-change-dev.json" || show_error "$auth_failed"
/usr/bin/rm -f -- "$data/license-remote-change.json" "$data/license-remote-change-dev.json" || show_error "$auth_failed"

if [ "$selection" = "$delete_label" ]; then
  # Recheck after authorization/removal; no cleanup occurs on failure or cancel.
  for path do
    safe_path "$path" || show_error "$cleanup_failed"
  done
  /usr/bin/rm -f -- "$config/graphs.json" "$config/audio-session.json" \
    "$data/recording-settings.json" || show_error "$cleanup_failed"
  /usr/bin/rm -rf -- "$config/graphs.states" "$data/WebKitCache" \
    "$data/MediaCache" "$data/applications" "$data/CacheStorage" || show_error "$cleanup_failed"
fi

/usr/bin/zenity --info --title="$title" --text="$done_message" || :
exit 0
}
