@echo off
setlocal
set job=C:\ToneNariLab\sysvad-bridge-audio-v11
if exist "%job%" exit /b 2
mkdir "%job%"
call :test > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:test
rem Native parent identity restricts this diagnostic to the separately installed bridge.
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ([Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) {throw 'Interactive session required'}; if ((Get-Content C:\ToneNariLab\sysvad-bridge-probe-build-v9\exit-code).Trim() -ne '0') {throw 'Probe build failed'}; $d=@(Get-CimInstance Win32_PnPEntity | Where-Object {$_.HardwareID -contains 'Root\tonenari_SysvadBridge'}); if ($d.Count -ne 1 -or $d[0].ConfigManagerErrorCode -ne 0) {throw 'Bridge not healthy'}; $nodes=@(Get-PnpDevice -Class AudioEndpoint -PresentOnly | Where-Object {(Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName DEVPKEY_Device_Parent).Data -eq $d[0].PNPDeviceID}); $nodes | Select-Object Status,FriendlyName,InstanceId | Format-List; $render=@($nodes | Where-Object {$_.InstanceId -eq 'SWD\MMDEVAPI\{0.0.0.00000000}.{1D1542D0-01AF-440A-AB03-A7C9728039F2}' -and $_.Status -eq 'OK'}); $capture=@($nodes | Where-Object {$_.InstanceId -eq 'SWD\MMDEVAPI\{0.0.1.00000000}.{41233BB0-E375-4072-B792-53341BB6F9AB}' -and $_.Status -eq 'OK'}); if ($render.Count -ne 1 -or $capture.Count -ne 1) {throw 'Explicit lab pair missing; refusing default device fallback'}; for ($i=0; $i -lt 20; $i++) { & C:\ToneNariLab\sysvad-bridge-probe-build-v9\SysvadBridgeProbe.exe --owner-exit; if ($LASTEXITCODE -ne 0) {throw 'Owner-exit probe failed'} }; Write-Output 'PASS: 20 controller exit/reattach cycles'; $prefix='SWD\MMDEVAPI\'; & C:\ToneNariLab\sysvad-bridge-probe-build-v9\SysvadBridgeProbe.exe $render[0].InstanceId.Substring($prefix.Length) $capture[0].InstanceId.Substring($prefix.Length); exit $LASTEXITCODE"
exit /b %errorlevel%
