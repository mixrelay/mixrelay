@echo off
setlocal
if exist C:\MixRelayLab\sysvad-audio-v1.log exit /b 2
call :probe > C:\MixRelayLab\sysvad-audio-v1.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\sysvad-audio-v1.exit-code echo %result%
exit /b %result%
:probe
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-Process -Id $PID).SessionId -eq 0) {throw 'Interactive session required'}; if ((Get-Content C:\MixRelayLab\sysvad-probe-build-v1\exit-code).Trim() -ne '0') {throw 'Probe build failed'}; foreach ($id in @('SWD\MMDEVAPI\{0.0.0.00000000}.{f9f48358-c9eb-4cde-9915-62265962bc5c}','SWD\MMDEVAPI\{0.0.1.00000000}.{16af4c64-2606-4edd-a632-3c76ed034eed}')) {if ((Get-PnpDeviceProperty -InstanceId $id -KeyName DEVPKEY_Device_Parent).Data -ne 'ROOT\MEDIA\0001') {throw 'Endpoint is not the installed sample'}; if ((Get-PnpDevice -InstanceId $id).Status -ne 'OK') {throw 'Sample endpoint unhealthy'}}"
if errorlevel 1 exit /b 3
C:\MixRelayLab\sysvad-probe-build-v1\SysvadAudioProbe.exe "{0.0.0.00000000}.{f9f48358-c9eb-4cde-9915-62265962bc5c}" "{0.0.1.00000000}.{16af4c64-2606-4edd-a632-3c76ed034eed}"
exit /b %errorlevel%
