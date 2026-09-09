@echo off
setlocal
set job=C:\ToneNariLab\sysvad-bridge-install-v1
set package=C:\ToneNariLab\sysvad-bridge-package-v1\package
if exist "%job%" exit /b 2
mkdir "%job%"
call :install > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:install
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-Content C:\ToneNariLab\sysvad-bridge-package-v1\exit-code).Trim() -ne '0') {throw 'Packaging failed'}; if (@(Get-CimInstance Win32_PnPEntity | Where-Object {$_.HardwareID -contains 'Root\tonenari_SysvadBridge'}).Count -ne 0) {throw 'Bridge exists; refusing duplicate installation'}; Add-Type -Path C:\Users\state303\CodeIntegrityState.cs; if ((Confirm-SecureBootUEFI) -or (([ToneNariCodeIntegrityState]::Read() -band 2) -eq 0)) {throw 'Unexpected signing state'}; foreach ($file in @('TabletAudioSample.sys','KeywordDetectorContosoAdapter.dll','sysvad.cat')) {$sig=Get-AuthenticodeSignature (Join-Path '%package%' $file); if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Thumbprint -ne 'BF2F641C417A264E7F51FC75A91E8AA19A87674D') {throw ('Invalid signature: '+$file)}}"
if errorlevel 1 exit /b 3
C:\ToneNariLab\wdk-arm64-26100\c\tools\10.0.26100.0\ARM64\devcon.exe install "%package%\ComponentizedAudioSample.inf" Root\tonenari_SysvadBridge
rem Original ToneNari and original SysVAD devices are not updated or removed.
exit /b %errorlevel%
