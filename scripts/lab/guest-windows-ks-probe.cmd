@echo off
setlocal
if exist C:\ToneNariLab\ks-probe-v2.log exit /b 2
call :inspect > C:\ToneNariLab\ks-probe-v2.log 2>&1
set result=%errorlevel%
>C:\ToneNariLab\ks-probe-v2.exit-code echo %result%
exit /b %result%
:inspect
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; Add-Type -Path C:\Users\state303\KsInterfaceProbe.cs; [ToneNariKsInterfaceProbe]::Run()"
if errorlevel 1 exit /b %errorlevel%
powershell.exe -NoProfile -NonInteractive -Command "Get-PnpDevice -Class AudioEndpoint | Format-List Status,FriendlyName,InstanceId; Get-CimInstance Win32_SoundDevice | Format-List Name,Status,PNPDeviceID"
exit /b %errorlevel%
