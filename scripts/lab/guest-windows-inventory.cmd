@echo off
chcp 65001 >nul
rem Read-only inventory through built-in commands. Do not change PowerShell
rem execution policy just to inspect a fresh guest; do not collect credentials.
ver
echo Process architecture: %PROCESSOR_ARCHITECTURE%
echo Native architecture hint: %PROCESSOR_ARCHITEW6432%
reg query "HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment" /v PROCESSOR_ARCHITECTURE
echo Guest agent:
sc query QEMU-GA
echo Display integration:
sc query vdservice
echo Toolchain paths in this execution account:
where winget
where git
where node
where cargo
where cl
rem Missing tools above are expected on a new guest, not an audio test result.
exit /b 0
