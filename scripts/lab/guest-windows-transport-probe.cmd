@echo off
setlocal
if exist C:\ToneNariLab\transport-probe-v1.log exit /b 2
call :probe > C:\ToneNariLab\transport-probe-v1.log 2>&1
set result=%errorlevel%
>C:\ToneNariLab\transport-probe-v1.exit-code echo %result%
exit /b %result%
:probe
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; Add-Type -Path C:\Users\state303\DriverTransportProbe.cs; $cycles=[ToneNariDriverTransportProbe]::Run(); Write-Output ('PASS: '+$cycles+' native transport attach/reject/cleanup cycles')"
exit /b %errorlevel%
