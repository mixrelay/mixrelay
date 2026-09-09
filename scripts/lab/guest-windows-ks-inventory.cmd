@echo off
setlocal
if exist C:\ToneNariLab\ks-inventory-v1.log exit /b 2
call :inspect > C:\ToneNariLab\ks-inventory-v1.log 2>&1
set result=%errorlevel%
>C:\ToneNariLab\ks-inventory-v1.exit-code echo %result%
exit /b %result%
:inspect
reg.exe query "HKLM\SYSTEM\CurrentControlSet\Control\DeviceClasses\{6994AD04-93EF-11D0-A3CC-00A0C9223196}" /s
pnputil.exe /enum-devices /instanceid "ROOT\MEDIA\0000" /interfaces /properties
exit /b %errorlevel%
