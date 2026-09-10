@echo off
setlocal
if not exist C:\MixRelayLab mkdir C:\MixRelayLab
(
  echo === Downloads ===
  dir C:\Users\state303\Downloads
  echo === Lab files ===
  dir C:\MixRelayLab
  dir C:\ProgramData\MixRelayLab
  echo === Installer processes ===
  tasklist /fi "imagename eq vs_installer.exe"
  echo === Installed tools ===
  where rustc
  where cargo
  where cl
  where node
  if exist "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -all -format json
) > C:\MixRelayLab\toolchain-inventory.log 2>&1
>C:\MixRelayLab\toolchain-inventory.exit-code echo 0
