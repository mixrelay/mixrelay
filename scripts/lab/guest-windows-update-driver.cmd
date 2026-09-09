@echo off
setlocal
set job=C:\ToneNariLab\update-driver-v2
set signed=C:\ToneNariLab\signed-driver-v2
if "%~1"=="v8" (
    set job=C:\ToneNariLab\update-driver-v3
    set signed=C:\ToneNariLab\signed-driver-v3
) else if not "%~1"=="" exit /b 2
if exist "%job%" exit /b 2
mkdir "%job%"
call :update > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:update
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-Content %signed%\exit-code).Trim() -ne '0') {throw 'Signing did not pass'}; $devices=@(Get-CimInstance Win32_PnPEntity | Where-Object {$_.HardwareID -contains 'Root\ToneNariVirtualAudio'}); if ($devices.Count -ne 1 -or $devices[0].PNPDeviceID -ne 'ROOT\MEDIA\0000') {throw 'Unexpected device set'}; $sig=Get-AuthenticodeSignature %signed%\package\ToneNariAudio.sys; if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Thumbprint -ne 'BF2F641C417A264E7F51FC75A91E8AA19A87674D') {throw 'Signature changed'}"
if errorlevel 1 exit /b 3
C:\ToneNariLab\wdk-arm64-26100\c\tools\10.0.26100.0\ARM64\devcon.exe update %signed%\package\ToneNariAudio.inf Root\ToneNariVirtualAudio
rem Keep native DevCon 0=success, 1=reboot required, 2+=failure distinguishable.
exit /b %errorlevel%
