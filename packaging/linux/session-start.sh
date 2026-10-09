#!/bin/sh
set -eu

case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  en*) instruction='Run MixRelay Driver as the signed-in user.' ;;
  ko*) instruction='로그인한 사용자로 MixRelay Driver를 실행하세요.' ;;
  ja*) instruction='ログイン中のユーザーとしてMixRelay Driverを実行してください。' ;;
  zh_Hans*|zh-Hans*) instruction='请以已登录的用户身份运行 MixRelay Driver。' ;;
  zh_Hant*|zh-Hant*|zh_TW*|zh-TW*|zh_HK*|zh-HK*|zh_MO*|zh-MO*) instruction='請以已登入的使用者身分執行 MixRelay Driver。' ;;
  zh*) instruction='请以已登录的用户身份运行 MixRelay Driver。' ;;
  fr*) instruction='Exécutez MixRelay Driver avec le compte utilisateur connecté.' ;;
  de*) instruction='Führen Sie MixRelay Driver als angemeldeter Benutzer aus.' ;;
  es*) instruction='Ejecute MixRelay Driver con el usuario que ha iniciado sesión.' ;;
  pt*) instruction='Execute o MixRelay Driver como o utilizador com sessão iniciada.' ;;
  it*) instruction='Eseguire MixRelay Driver con l’utente che ha effettuato l’accesso.' ;;
  ru*) instruction='Запустите MixRelay Driver от имени пользователя, вошедшего в систему.' ;;
  *) instruction='Run MixRelay Driver as the signed-in user.' ;;
esac
[ "$(id -u)" -ne 0 ] || { printf '%s\n' "$instruction" >&2; exit 1; }
systemctl --user start mixrelay-driver.service
case "${1:-}" in
  '') ;;
  --manager) exec /usr/bin/mixrelay-driver-manager ;;
  *) exit 2 ;;
esac
