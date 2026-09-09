@echo off
setlocal
rem A distinct diagnostic bridge. Never overwrite upstream baseline evidence.
if "%~1"=="" exit /b 2
powershell.exe -NoProfile -NonInteractive -Command "if ('%~1' -notmatch '^sysvad-bridge-build-v[0-9]+$') {exit 2}"
if errorlevel 1 exit /b 2
set job=C:\ToneNariLab\%~1
if exist "%job%" exit /b 2
mkdir "%job%"
call :build > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:build
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-FileHash C:\ToneNariLab\sysvad-bridge-v12.tar.gz).Hash -ne '278C0B193CBF64ACFE9EA68FE8DB0DAA9D7C57A61F152202765CE60484066806') {throw 'Bridge source hash mismatch'}"
if errorlevel 1 exit /b 3
call C:\ToneNariLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
mkdir "%job%\source"
tar.exe -xzf C:\ToneNariLab\sysvad-bridge-v12.tar.gz -C "%job%\source"
if errorlevel 1 exit /b 5
cd /d "%job%\source\audio\sysvad"
msbuild.exe EndpointsCommon\EndpointsCommon.vcxproj /m:2 /p:Configuration=Release /p:Platform=ARM64 /p:WindowsTargetPlatformVersion=10.0.26100.0 /p:SignMode=Off /v:minimal /bl:"%job%\endpoints.binlog"
if errorlevel 1 exit /b 6
msbuild.exe TabletAudioSample\TabletAudioSample.vcxproj /m:2 /p:Configuration=Release /p:Platform=ARM64 /p:WindowsTargetPlatformVersion=10.0.26100.0 /p:SignMode=Off /v:minimal /bl:"%job%\driver.binlog"
if errorlevel 1 exit /b 7
echo PASS: bridge compiled. Native PCM and installation are separate gates.
exit /b 0
