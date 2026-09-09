@echo off
setlocal
set job=C:\ToneNariLab\sysvad-bridge-update-v3
set package=C:\ToneNariLab\sysvad-bridge-package-v5\package
if exist "%job%" exit /b 2
mkdir "%job%"
call :update > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:update
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-Content C:\ToneNariLab\sysvad-bridge-package-v5\exit-code).Trim() -ne '0') {throw 'Packaging failed'}; $d=@(Get-CimInstance Win32_PnPEntity | Where-Object {$_.HardwareID -contains 'Root\tonenari_SysvadBridge'}); if ($d.Count -ne 1 -or $d[0].PNPDeviceID -ne 'ROOT\MEDIA\0002') {throw 'Refusing update: exact lab device identity mismatch'}; Add-Type -Path C:\Users\state303\CodeIntegrityState.cs; if ((Confirm-SecureBootUEFI) -or (([ToneNariCodeIntegrityState]::Read() -band 2) -eq 0)) {throw 'Unexpected signing state'}; foreach ($file in @('TabletAudioSample.sys','KeywordDetectorContosoAdapter.dll','sysvad.cat')) {$sig=Get-AuthenticodeSignature (Join-Path '%package%' $file); if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Thumbprint -ne 'BF2F641C417A264E7F51FC75A91E8AA19A87674D') {throw ('Signature mismatch: '+$file)}}; $d | Select-Object Name,PNPDeviceID,ConfigManagerErrorCode | Format-List"
if errorlevel 1 exit /b 3
C:\ToneNariLab\wdk-arm64-26100\c\tools\10.0.26100.0\ARM64\devcon.exe update "%package%\ComponentizedAudioSample.inf" Root\tonenari_SysvadBridge
exit /b %errorlevel%
