@echo off
setlocal
rem Install only the exact test-signed artifact in the explicitly authorized VM.
set job=C:\ToneNariLab\load-driver-v1
if exist "%job%" exit /b 2
mkdir "%job%"
call :load > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:load
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized Windows VM'}; if ((Get-Content C:\ToneNariLab\signed-driver-v1\exit-code).Trim() -ne '0') {throw 'Signing validation incomplete'}; Add-Type -Path C:\Users\state303\CodeIntegrityState.cs; if (([ToneNariCodeIntegrityState]::Read() -band 2) -eq 0) {throw 'Test signing inactive'}; if ((Get-FileHash C:\ToneNariLab\signed-driver-v1\package\ToneNariAudio.sys).Hash -ne '5104C03BF44A1531BBF3C502A2951A6E40E2A04EEAB6A7BD43AA7E0A79847D13') {throw 'Driver binary changed'}; if (Get-CimInstance Win32_PnPEntity | Where-Object {$_.HardwareID -contains 'Root\ToneNariVirtualAudio'}) {throw 'ToneNari device already exists; do not create duplicate'}; Get-CimInstance Win32_SoundDevice | Select-Object Name,Status,PNPDeviceID | Format-List"
if errorlevel 1 exit /b 3
C:\ToneNariLab\wdk-arm64-26100\c\tools\10.0.26100.0\ARM64\devcon.exe install C:\ToneNariLab\signed-driver-v1\package\ToneNariAudio.inf Root\ToneNariVirtualAudio
if errorlevel 2 exit /b 4
if errorlevel 1 exit /b 5
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; $devices=@(Get-CimInstance Win32_PnPEntity | Where-Object {$_.HardwareID -contains 'Root\ToneNariVirtualAudio'}); $devices | Select-Object Name,PNPDeviceID,Status,ConfigManagerErrorCode | Format-List; $driver=Get-CimInstance Win32_SystemDriver -Filter 'Name=''ToneNariAudio'''; $driver | Select-Object Name,State,Started,PathName | Format-List; Get-PnpDevice -Class AudioEndpoint | Where-Object FriendlyName -like '*ToneNari*' | Select-Object Status,FriendlyName,InstanceId | Format-List; if ($devices.Count -ne 1 -or $devices[0].ConfigManagerErrorCode -ne 0 -or -not $driver.Started) {throw 'Native PnP/driver load did not succeed'}"
if errorlevel 1 exit /b 6
echo PASS: ToneNari kernel driver started and native device has no PnP error. PCM remains unverified.
exit /b 0
