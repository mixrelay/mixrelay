@echo off
setlocal
if exist C:\MixRelayLab\driver-inventory-v1.log exit /b 2
call :inspect > C:\MixRelayLab\driver-inventory-v1.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\driver-inventory-v1.exit-code echo %result%
exit /b %result%
:inspect
sc.exe query AudioEndpointBuilder
sc.exe query Audiosrv
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; Get-PnpDevice -Class AudioEndpoint | Select-Object Status,FriendlyName,InstanceId | Format-List; Get-CimInstance Win32_SoundDevice | Select-Object Name,Status,PNPDeviceID | Format-List; Get-WinEvent -FilterHashtable @{LogName='System'; StartTime=(Get-Date).AddMinutes(-15); Level=1,2,3} -ErrorAction SilentlyContinue | Select-Object -First 15 TimeCreated,ProviderName,Id,Message | Format-List"
pnputil.exe /enum-devices /instanceid "ROOT\MEDIA\0000" /interfaces /drivers
exit /b %errorlevel%
