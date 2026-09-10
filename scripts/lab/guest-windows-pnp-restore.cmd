@echo off
setlocal
if exist C:\MixRelayLab\pnp-restore-v1 exit /b 2
mkdir C:\MixRelayLab\pnp-restore-v1
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\MixRelayLab\windows-lab-pnp-restore.ps1 > C:\MixRelayLab\pnp-restore-v1\output.log 2>&1
set result=%errorlevel%
call C:\MixRelayLab\pnp-final-inspect.cmd
>C:\MixRelayLab\pnp-restore-v1\exit-code echo %result%
exit /b %result%
