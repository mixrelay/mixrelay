@echo off
setlocal
rem Read-only, no certificate or boot-policy changes.
if exist C:\ToneNariLab\signing-state-v1.log exit /b 2
call :inspect > C:\ToneNariLab\signing-state-v1.log 2>&1
set result=%errorlevel%
>C:\ToneNariLab\signing-state-v1.exit-code echo %result%
exit /b %result%
:inspect
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; [pscustomobject]@{SecureBoot=(Confirm-SecureBootUEFI); UserName=(Get-CimInstance Win32_ComputerSystem).UserName; UUID=(Get-CimInstance Win32_ComputerSystemProduct).UUID} | ConvertTo-Json"
if errorlevel 1 exit /b 3
bcdedit.exe /enum {current}
if errorlevel 1 exit /b 4
exit /b 0
