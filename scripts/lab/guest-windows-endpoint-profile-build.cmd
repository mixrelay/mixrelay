@echo off
setlocal
rem Native 64-render/16-capture descriptor qualification. Build only, no installation.
set job=C:\MixRelayLab\endpoint-profile-build-v1
if exist "%job%" exit /b 2
mkdir "%job%"
call :build > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:build
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-FileHash C:\MixRelayLab\sysvad-profile-64-16-v2.tar.gz).Hash -ne '1F2D576FD63A63BF001E0C1B0EA4B97FDEF80657E028B2F74365F6812F39661B') {throw 'Profile source hash mismatch'}"
if errorlevel 1 exit /b 3
call C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
mkdir "%job%\source"
tar.exe -xzf C:\MixRelayLab\sysvad-profile-64-16-v2.tar.gz -C "%job%\source"
if errorlevel 1 exit /b 5
type "%job%\source\MIXRELAY-ENDPOINT-PROFILE.json"
cd /d "%job%\source\audio\sysvad"
msbuild.exe EndpointsCommon\EndpointsCommon.vcxproj /m:2 /p:Configuration=Release /p:Platform=ARM64 /p:WindowsTargetPlatformVersion=10.0.26100.0 /p:SignMode=Off /v:minimal /bl:"%job%\endpoints.binlog"
if errorlevel 1 exit /b 6
msbuild.exe TabletAudioSample\TabletAudioSample.vcxproj /m:2 /p:Configuration=Release /p:Platform=ARM64 /p:WindowsTargetPlatformVersion=10.0.26100.0 /p:SignMode=Off /v:minimal /bl:"%job%\driver.binlog"
if errorlevel 1 exit /b 7
echo PASS: ARM64 64/16 native descriptors compiled. No installation, PCM or dynamic reconfiguration claim.
exit /b 0
