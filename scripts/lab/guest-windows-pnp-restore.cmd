@echo off
setlocal
if exist C:\ToneNariLab\pnp-restore-v1 exit /b 2
mkdir C:\ToneNariLab\pnp-restore-v1
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\ToneNariLab\windows-lab-pnp-restore.ps1 > C:\ToneNariLab\pnp-restore-v1\output.log 2>&1
set result=%errorlevel%
call C:\ToneNariLab\pnp-final-inspect.cmd
>C:\ToneNariLab\pnp-restore-v1\exit-code echo %result%
exit /b %result%
