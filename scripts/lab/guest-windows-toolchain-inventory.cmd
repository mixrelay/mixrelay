@echo off
setlocal
if not exist C:\ToneNariLab mkdir C:\ToneNariLab
(
  echo === Downloads ===
  dir C:\Users\state303\Downloads
  echo === Lab files ===
  dir C:\ToneNariLab
  dir C:\ProgramData\ToneNariLab
  echo === Installer processes ===
  tasklist /fi "imagename eq vs_installer.exe"
  echo === Installed tools ===
  where rustc
  where cargo
  where cl
  where node
  if exist "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -all -format json
) > C:\ToneNariLab\toolchain-inventory.log 2>&1
>C:\ToneNariLab\toolchain-inventory.exit-code echo 0
