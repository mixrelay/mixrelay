@echo off
setlocal
set job=C:\ToneNariLab\sysvad-bridge-package-v5
set source=C:\ToneNariLab\sysvad-bridge-build-v8\source\audio\sysvad\TabletAudioSample\ARM64\Release
if exist "%job%" exit /b 2
mkdir "%job%"
call :package > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:package
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-Content C:\ToneNariLab\sysvad-bridge-build-v8\exit-code).Trim() -ne '0') {throw 'Bridge build failed'}; Add-Type -Path C:\Users\state303\CodeIntegrityState.cs; if ((Confirm-SecureBootUEFI) -or (([ToneNariCodeIntegrityState]::Read() -band 2) -eq 0)) {throw 'Unexpected signing state'}; $c=Get-Item Cert:\LocalMachine\My\BF2F641C417A264E7F51FC75A91E8AA19A87674D; if (-not $c.HasPrivateKey -or $c.NotAfter -le (Get-Date)) {throw 'Certificate unavailable'}; $inf=Get-Content '%source%\ComponentizedAudioSample.inf' -Raw; if (!$inf.Contains('Root\tonenari_SysvadBridge') -or $inf.Contains('Root\sysvad_ComponentizedAudioSample')) {throw 'Bridge identity mismatch'}; $b=[IO.File]::ReadAllBytes('%source%\TabletAudioSample.sys'); $pe=[BitConverter]::ToInt32($b,60); if (([BitConverter]::ToUInt16($b,$pe+94) -band 128) -eq 0) {throw 'FORCE_INTEGRITY missing'}"
if errorlevel 1 exit /b 3
mkdir "%job%\package"
copy "%source%\TabletAudioSample.sys" "%job%\package\TabletAudioSample.sys"
if errorlevel 1 exit /b 4
copy "%source%\ComponentizedAudioSample.inf" "%job%\package\ComponentizedAudioSample.inf"
if errorlevel 1 exit /b 5
copy C:\ToneNariLab\sysvad-package-v2\package\KeywordDetectorContosoAdapter.dll "%job%\package\KeywordDetectorContosoAdapter.dll"
if errorlevel 1 exit /b 6
set signer=C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\arm64\signtool.exe
"%signer%" sign /v /fd SHA256 /sm /s My /sha1 BF2F641C417A264E7F51FC75A91E8AA19A87674D "%job%\package\TabletAudioSample.sys"
if errorlevel 1 exit /b 7
rem Universal diagnostic sample only: /w isolation remains an unmet product gate.
C:\ToneNariLab\wdk-arm64-26100\c\tools\10.0.26100.0\ARM64\infverif.exe /u /v "%job%\package\ComponentizedAudioSample.inf"
if errorlevel 1 exit /b 8
C:\ToneNariLab\wdk-arm64-26100\c\bin\10.0.26100.0\x86\Inf2Cat.exe /driver:"%job%\package" /os:10_GE_ARM64 /verbose
if errorlevel 1 exit /b 9
"%signer%" sign /v /fd SHA256 /sm /s My /sha1 BF2F641C417A264E7F51FC75A91E8AA19A87674D "%job%\package\sysvad.cat"
if errorlevel 1 exit /b 10
"%signer%" verify /v /pa /c "%job%\package\sysvad.cat" "%job%\package\TabletAudioSample.sys" "%job%\package\KeywordDetectorContosoAdapter.dll"
if errorlevel 1 exit /b 11
powershell.exe -NoProfile -NonInteractive -Command "Get-FileHash %job%\package\* | Format-List"
exit /b %errorlevel%
