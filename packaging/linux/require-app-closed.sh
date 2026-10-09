#!/bin/sh
set -eu

case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  en*) instruction='Quit MixRelay from its menu or tray, then retry the installation, update or removal.' ;;
  ko*) instruction='MixRelay 메뉴나 트레이에서 종료한 뒤 설치, 업데이트 또는 제거를 다시 시도하세요.' ;;
  ja*) instruction='MixRelayのメニューまたはトレイから終了し、インストール、更新、削除を再試行してください。' ;;
  zh_Hans*|zh-Hans*) instruction='请从菜单或托盘退出 MixRelay，然后重试安装、更新或移除。' ;;
  zh_Hant*|zh-Hant*|zh_TW*|zh-TW*|zh_HK*|zh-HK*|zh_MO*|zh-MO*) instruction='請從選單或系統匣結束 MixRelay，然後重試安裝、更新或移除。' ;;
  zh*) instruction='请从菜单或托盘退出 MixRelay，然后重试安装、更新或移除。' ;;
  fr*) instruction='Quittez MixRelay depuis son menu ou la barre d’état, puis réessayez l’installation, la mise à jour ou la suppression.' ;;
  de*) instruction='Beenden Sie MixRelay über das Menü oder die Statusleiste und wiederholen Sie die Installation, das Update oder die Entfernung.' ;;
  es*) instruction='Cierre MixRelay desde su menú o bandeja y vuelva a intentar la instalación, actualización o eliminación.' ;;
  pt*) instruction='Feche o MixRelay pelo menu ou pela área de notificação e tente instalar, atualizar ou remover novamente.' ;;
  it*) instruction='Chiudere MixRelay dal menu o dall’area di notifica, quindi riprovare l’installazione, l’aggiornamento o la rimozione.' ;;
  ru*) instruction='Закройте MixRelay через меню или системный лоток, затем повторите установку, обновление или удаление.' ;;
  *) instruction='Quit MixRelay from its menu or tray, then retry the installation, update or removal.' ;;
esac

# The installed desktop owns the interactive engine. Let the user save and quit
# before the package manager replaces or removes either executable.
for executable in /proc/[0-9]*/exe; do
  target=$(readlink "$executable" 2>/dev/null) || continue
  case "$target" in
    /usr/bin/mixrelay-desktop|'/usr/bin/mixrelay-desktop (deleted)'|\
    /usr/bin/mixrelay-engine-service|'/usr/bin/mixrelay-engine-service (deleted)')
      printf '%s\n' "$instruction" >&2
      exit 1
      ;;
  esac
done

exit 0
