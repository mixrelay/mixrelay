@echo off
setlocal
set job=C:\MixRelayLab\sysvad-bridge-probe-build-v11
if exist "%job%" exit /b 2
mkdir "%job%"
call :build > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:build
powershell.exe -NoProfile -NonInteractive -Command "if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {exit 2}"
if errorlevel 1 exit /b 3
call C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
cd /d "%job%"
cl.exe /nologo /std:c++17 /EHsc /W4 /WX /O2 /I C:\Users\state303 C:\Users\state303\SysvadBridgeProbe.cpp /Fe:SysvadBridgeProbe.exe /link ole32.lib uuid.lib
exit /b %errorlevel%
