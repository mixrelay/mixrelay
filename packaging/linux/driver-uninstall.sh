#!/bin/sh

{
set -eu
case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  ko*) title='MixRelay 제거'; message='MixRelay의 가상 장치와 서비스를 제거하며 시스템 오디오가 잠시 끊깁니다.'; delete_label='저장된 MixRelay 장치 설정 삭제'; failed='MixRelay를 제거하지 못했습니다. MixRelay를 종료한 뒤 다시 시도하세요.'; auth_cancelled='인증이 취소되어 MixRelay 패키지를 제거하지 않았습니다. 저장된 설정은 삭제하지 않았습니다.'; root_error='로그인한 일반 사용자로 MixRelay 제거를 실행하세요.'; auth_failed='승인 불가 또는 인증 오류로 MixRelay 패키지를 제거하지 않았습니다. 저장된 설정은 삭제하지 않았습니다.' ;;
  ja*)
    title='MixRelay のアンインストール'
    message='MixRelayの仮想デバイスとサービスを削除し、システム音声を一時的に中断します。'
    delete_label='保存された MixRelay デバイス設定を削除'
    failed='MixRelay を削除できませんでした。MixRelay と MixRelay 管理アプリを終了してから再試行してください。'
    auth_cancelled='認証がキャンセルされました。MixRelay パッケージは削除されず、保存された設定も削除されていません。'
    auth_failed='認証を利用できないか、認証に失敗しました。MixRelay パッケージは削除されず、保存された設定も削除されていません。'
    root_error='ログイン中の一般ユーザーとして MixRelay の削除を実行してください。'
    ;;
  zh_Hans*|zh-Hans*)
    title='卸载 MixRelay'
    message='移除 MixRelay 的虚拟设备和服务，并暂时中断系统音频。'
    delete_label='删除保存的 MixRelay 设备设置'
    failed='MixRelay 移除失败。请退出 MixRelay 和 MixRelay 管理应用后重试。'
    auth_cancelled='认证已取消。MixRelay 软件包未移除，保存的设置未删除。'
    auth_failed='无法授权或认证失败。MixRelay 软件包未移除，保存的设置未删除。'
    root_error='请以已登录的普通用户身份移除 MixRelay。'
    ;;
  zh_Hant*|zh-Hant*|zh_TW*|zh-TW*|zh_HK*|zh-HK*|zh_MO*|zh-MO*)
    title='解除安裝 MixRelay'
    message='移除 MixRelay 的虛擬裝置與服務，並暫時中斷系統音訊。'
    delete_label='刪除儲存的 MixRelay 裝置設定'
    failed='MixRelay 移除失敗。請結束 MixRelay 與 MixRelay 管理應用程式後重試。'
    auth_cancelled='驗證已取消。MixRelay 套件未移除，儲存的設定未刪除。'
    auth_failed='無法授權或驗證失敗。MixRelay 套件未移除，儲存的設定未刪除。'
    root_error='請以已登入的一般使用者身分移除 MixRelay。'
    ;;
  zh*)
    title='卸载 MixRelay'
    message='移除 MixRelay 的虚拟设备和服务，并暂时中断系统音频。'
    delete_label='删除保存的 MixRelay 设备设置'
    failed='MixRelay 移除失败。请退出 MixRelay 和 MixRelay 管理应用后重试。'
    auth_cancelled='认证已取消。MixRelay 软件包未移除，保存的设置未删除。'
    auth_failed='无法授权或认证失败。MixRelay 软件包未移除，保存的设置未删除。'
    root_error='请以已登录的普通用户身份移除 MixRelay。'
    ;;
  fr*)
    title='Désinstaller MixRelay'
    message='Supprime les périphériques virtuels et le service de MixRelay, et interrompt brièvement l’audio du système.'
    delete_label='Supprimer les paramètres de périphériques MixRelay enregistrés'
    failed='La suppression de MixRelay a échoué. Quittez MixRelay et le gestionnaire MixRelay, puis réessayez.'
    auth_cancelled='L’authentification a été annulée. Le paquet MixRelay n’a pas été supprimé et les paramètres enregistrés n’ont pas été effacés.'
    auth_failed='L’autorisation est indisponible ou l’authentification a échoué. Le paquet MixRelay n’a pas été supprimé et les paramètres enregistrés n’ont pas été effacés.'
    root_error='Exécutez la suppression de MixRelay avec le compte utilisateur ordinaire connecté.'
    ;;
  de*)
    title='MixRelay deinstallieren'
    message='Entfernt die virtuellen Geräte und den Dienst von MixRelay und unterbricht kurz das Systemaudio.'
    delete_label='Gespeicherte MixRelay-Geräteeinstellungen löschen'
    failed='Der MixRelay konnte nicht entfernt werden. Beenden Sie MixRelay und den MixRelay Manager und versuchen Sie es erneut.'
    auth_cancelled='Die Authentifizierung wurde abgebrochen. Das MixRelay-Paket wurde nicht entfernt und gespeicherte Einstellungen wurden nicht gelöscht.'
    auth_failed='Die Autorisierung ist nicht verfügbar oder die Authentifizierung ist fehlgeschlagen. Das MixRelay-Paket wurde nicht entfernt und gespeicherte Einstellungen wurden nicht gelöscht.'
    root_error='Führen Sie die MixRelay-Entfernung als angemeldeter regulärer Benutzer aus.'
    ;;
  es*)
    title='Desinstalar MixRelay'
    message='Elimina los dispositivos virtuales y el servicio de MixRelay e interrumpe brevemente el audio del sistema.'
    delete_label='Eliminar la configuración guardada de los dispositivos de MixRelay'
    failed='No se pudo eliminar MixRelay. Cierre MixRelay y el administrador de MixRelay e inténtelo de nuevo.'
    auth_cancelled='Se canceló la autenticación. No se eliminó el paquete MixRelay ni la configuración guardada.'
    auth_failed='La autorización no está disponible o la autenticación falló. No se eliminó el paquete MixRelay ni la configuración guardada.'
    root_error='Ejecute la eliminación de MixRelay con el usuario normal que ha iniciado sesión.'
    ;;
  pt*)
    title='Desinstalar MixRelay'
    message='Remove os dispositivos virtuais e o serviço do MixRelay e interrompe brevemente o áudio do sistema.'
    delete_label='Eliminar definições guardadas dos dispositivos MixRelay'
    failed='A remoção do MixRelay falhou. Feche o MixRelay e o gestor do MixRelay e tente novamente.'
    auth_cancelled='A autenticação foi cancelada. O pacote MixRelay não foi removido e as definições guardadas não foram eliminadas.'
    auth_failed='A autorização não está disponível ou a autenticação falhou. O pacote MixRelay não foi removido e as definições guardadas não foram eliminadas.'
    root_error='Execute a remoção do MixRelay como o utilizador normal com sessão iniciada.'
    ;;
  it*)
    title='Disinstallare MixRelay'
    message='Rimuove i dispositivi virtuali e il servizio di MixRelay e interrompe brevemente l’audio del sistema.'
    delete_label='Eliminare le impostazioni salvate dei dispositivi MixRelay'
    failed='Rimozione di MixRelay non riuscita. Chiudere MixRelay e il gestore di MixRelay, quindi riprovare.'
    auth_cancelled='Autenticazione annullata. Il pacchetto MixRelay non è stato rimosso e le impostazioni salvate non sono state eliminate.'
    auth_failed='Autorizzazione non disponibile o autenticazione non riuscita. Il pacchetto MixRelay non è stato rimosso e le impostazioni salvate non sono state eliminate.'
    root_error='Eseguire la rimozione di MixRelay con l’utente normale che ha effettuato l’accesso.'
    ;;
  ru*)
    title='Удалить MixRelay'
    message='Удаляет виртуальные устройства и службу MixRelay и ненадолго прерывает системный звук.'
    delete_label='Удалить сохранённые настройки устройств MixRelay'
    failed='Не удалось удалить MixRelay. Закройте MixRelay и приложение управления MixRelay, затем повторите попытку.'
    auth_cancelled='Авторизация отменена. Пакет MixRelay и сохранённые настройки не удалены.'
    auth_failed='Разрешение недоступно или авторизация не удалась. Пакет MixRelay и сохранённые настройки не удалены.'
    root_error='Запустите удаление MixRelay от имени обычного пользователя, вошедшего в систему.'
    ;;
  *) title='Uninstall MixRelay'; message='Removes the virtual devices and service of MixRelay and briefly interrupts system audio.'; delete_label='Delete saved MixRelay device settings'; failed='MixRelay removal failed. Quit MixRelay, then retry.'; auth_cancelled='Authentication cancelled. The MixRelay package was not removed and saved settings were not deleted.'; root_error='Run MixRelay removal as the signed-in regular user.'; auth_failed='Authorization unavailable or authentication failed. The MixRelay package was not removed and saved settings were not deleted.' ;;
esac
[ "$(id -u)" -ne 0 ] || { printf '%s\n' "$root_error" >&2; exit 1; }
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
stopped="$settings.stopped"
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
[ ! -L "$stopped" ] && [ ! -d "$stopped" ] || exit 1
selection=$(zenity --list --checklist --title="$title" --text="$message" \
  --column='' --column='' FALSE "$delete_label") || exit 0
if command -v dpkg-query >/dev/null 2>&1 && dpkg-query -W -f='${db:Status-Status}' mix-relay-driver 2>/dev/null | grep -Eq '^(installed|unpacked|half-configured|half-installed|triggers-awaited|triggers-pending)$'; then
  remove_package /usr/bin/apt-get -y remove -- mix-relay-driver
elif command -v rpm >/dev/null 2>&1 && rpm -q mix-relay-driver >/dev/null 2>&1; then
  remove_package /usr/bin/dnf -y --setopt=clean_requirements_on_remove=False remove mix-relay-driver
elif command -v pacman >/dev/null 2>&1 && pacman -Q mix-relay-driver >/dev/null 2>&1; then
  remove_package /usr/bin/pacman --noconfirm -R -- mix-relay-driver
else
  zenity --error --title="$title" --text="$failed"
  exit 1
fi
if [ "$selection" = "$delete_label" ]; then
  # User privileges and a fixed regular filename: never recursively remove a directory.
  [ ! -d "$settings" ] && [ ! -L "$settings" ] || exit 1
  [ ! -d "$stopped" ] && [ ! -L "$stopped" ] || exit 1
  rm -f -- "$settings" "$stopped"
fi
}
