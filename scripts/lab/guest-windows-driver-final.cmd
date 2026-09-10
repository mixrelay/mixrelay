@echo off
setlocal
set job=C:\MixRelayLab\driver-final-v2
set signed=C:\MixRelayLab\signed-driver-v2
if "%~1"=="v8" (
    set job=C:\MixRelayLab\driver-final-v3
    set signed=C:\MixRelayLab\signed-driver-v3
) else if not "%~1"=="" exit /b 2
if exist %job%.log exit /b 2
call :verify > %job%.log 2>&1
set result=%errorlevel%
>%job%.exit-code echo %result%
exit /b %result%
:verify
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; Add-Type -Path C:\Users\state303\CodeIntegrityState.cs; $ci=[MixRelayCodeIntegrityState]::Read(); if ((Confirm-SecureBootUEFI) -or ($ci -band 2) -eq 0) {throw 'Unexpected boot policy'}; $driver=Get-CimInstance Win32_SystemDriver -Filter 'Name=''MixRelayAudio'''; $driver | Select-Object Name,State,Started,PathName | Format-List; if (-not $driver.Started) {throw 'Driver not started'}; $expected=(Get-FileHash %signed%\package\MixRelayAudio.sys).Hash; $installed=(Get-FileHash $driver.PathName).Hash; if ($expected -ne $installed) {throw 'Installed driver binary does not match selected signed build'}; Write-Output ('PASS: running test-signing kernel; installed selected signed build SHA256='+$installed); Get-CimInstance Win32_PnPEntity | Where-Object {$_.HardwareID -contains 'Root\MixRelayVirtualAudio'} | Select-Object Name,PNPDeviceID,ConfigManagerErrorCode | Format-List; Add-Type -Path C:\Users\state303\DriverTransportProbe.cs; $cycles=[MixRelayDriverTransportProbe]::Run(); Write-Output ('PASS: '+$cycles+' native transport attach/reject/cleanup cycles'); $endpoints=@(Get-PnpDevice -Class AudioEndpoint -PresentOnly | Where-Object FriendlyName -like '*MixRelay*'); $endpoints | Select-Object Status,FriendlyName,InstanceId | Format-List; if ($endpoints.Count -ne 2 -or @($endpoints | Where-Object Status -ne 'OK').Count -ne 0) {throw 'Expected two healthy MixRelay audio endpoints'}; Write-Output 'PASS: two native MixRelay audio endpoints; actual audio PCM still requires testing'"
exit /b %errorlevel%
