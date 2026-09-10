@echo off
setlocal
rem Read-only inventory, kept separate from certificate/BCD mutation.
if exist C:\MixRelayLab\signing-inspect-v1.log exit /b 2
call :inspect > C:\MixRelayLab\signing-inspect-v1.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\signing-inspect-v1.exit-code echo %result%
exit /b %result%
:inspect
whoami
powershell.exe -NoProfile -NonInteractive -Command "Get-CimInstance Win32_ComputerSystemProduct | Select-Object UUID,Vendor,Name; Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version; try {Confirm-SecureBootUEFI} catch {$_.Exception.Message}; Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Filter signtool.exe -Recurse | Select-Object FullName"
bcdedit.exe /enum {current}
if errorlevel 1 exit /b 3
manage-bde.exe -status C:
if errorlevel 1 exit /b 4
exit /b 0
