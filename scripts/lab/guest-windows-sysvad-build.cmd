@echo off
setlocal
rem Build the pinned upstream sample, not the product driver. No signing/install.
if "%~1"=="" exit /b 2
powershell.exe -NoProfile -NonInteractive -Command "if ('%~1' -notmatch '^sysvad-build-v[0-9]+$') {exit 2}"
if errorlevel 1 exit /b 2
set job=C:\MixRelayLab\%~1
if exist "%job%" exit /b 2
mkdir "%job%"
call :build > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:build
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-Content C:\MixRelayLab\sysvad-prepare-v1.exit-code).Trim() -ne '0') {throw 'SDK preparation did not pass'}; if ((Get-FileHash C:\MixRelayLab\sysvad-upstream.tar.gz).Hash -ne '3B8EF2C32F43348CAA99623BA7930089B29386CC3D838F119E902F9FD8FC554D') {throw 'Upstream source archive changed'}"
if errorlevel 1 exit /b 3
call C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
mkdir "%job%\source"
tar.exe -xzf C:\MixRelayLab\sysvad-upstream.tar.gz -C "%job%\source"
if errorlevel 1 exit /b 5
copy C:\Users\state303\sysvad-build.props "%job%\source\Directory.Build.props"
if errorlevel 1 exit /b 6
cd /d "%job%\source\audio\sysvad"
msbuild.exe EndpointsCommon\EndpointsCommon.vcxproj /m:2 /p:Configuration=Release /p:Platform=ARM64 /p:WindowsTargetPlatformVersion=10.0.26100.0 /p:SignMode=Off /v:minimal /bl:"%job%\endpoints.binlog"
if errorlevel 1 exit /b 7
msbuild.exe TabletAudioSample\TabletAudioSample.vcxproj /m:2 /p:Configuration=Release /p:Platform=ARM64 /p:WindowsTargetPlatformVersion=10.0.26100.0 /p:SignMode=Off /v:minimal /bl:"%job%\driver.binlog"
if errorlevel 1 exit /b 8
echo PASS: upstream SysVAD native ARM64 build. Not installed or audio-qualified.
exit /b 0
