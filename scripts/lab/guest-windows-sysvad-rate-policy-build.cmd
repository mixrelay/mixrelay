@echo off
setlocal
rem Build only. No signing, installation, PnP or audio-default changes.
if "%~3"=="" exit /b 2
powershell.exe -NoProfile -NonInteractive -Command "if ('%~1' -notmatch '^sysvad-rate-policy-build-v[0-9]+$' -or '%~2' -notmatch '^sysvad-rate-policy-v[0-9]+\.tar\.gz$' -or '%~3' -notmatch '^[A-Fa-f0-9]{64}$') {exit 2}"
if errorlevel 1 exit /b 2
set job=C:\ToneNariLab\%~1
if exist "%job%" exit /b 2
mkdir "%job%"
set archive=%~2
set archiveHash=%~3
call :build > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:build
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-FileHash ('C:\ToneNariLab\'+$env:archive)).Hash -ne $env:archiveHash) {throw 'Source hash mismatch'}"
if errorlevel 1 exit /b 3
call C:\ToneNariLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
mkdir "%job%\source"
tar.exe -xzf "C:\ToneNariLab\%archive%" -C "%job%\source"
if errorlevel 1 exit /b 5
type "%job%\source\TONENARI-RATE-POLICY.json"
if errorlevel 1 exit /b 5
cd /d "%job%\source\audio\sysvad"
msbuild.exe EndpointsCommon\EndpointsCommon.vcxproj /m:2 /p:Configuration=Release /p:Platform=ARM64 /p:WindowsTargetPlatformVersion=10.0.26100.0 /p:SignMode=Off /v:minimal /bl:"%job%\endpoints.binlog"
if errorlevel 1 exit /b 6
msbuild.exe TabletAudioSample\TabletAudioSample.vcxproj /m:2 /p:Configuration=Release /p:Platform=ARM64 /p:WindowsTargetPlatformVersion=10.0.26100.0 /p:SignMode=Off /v:minimal /bl:"%job%\driver.binlog"
if errorlevel 1 exit /b 7
cl.exe /nologo /std:c++17 /W4 /WX /EHsc /Iinclude rate-policy-probe.cpp /Fe:"%job%\rate-policy-probe.exe"
if errorlevel 1 exit /b 8
for %%t in (rate_policy ring pcm) do (
    cl.exe /nologo /std:c++17 /W4 /WX /EHsc /Iinclude "ToneNariTests\%%t_test.cpp" /Fe:"%job%\%%t-test.exe"
    if errorlevel 1 exit /b 9
    "%job%\%%t-test.exe"
    if errorlevel 1 exit /b 10
)
echo PASS: SysVAD policy ARM64 source built. Native renegotiation is NOT yet qualified.
exit /b 0
