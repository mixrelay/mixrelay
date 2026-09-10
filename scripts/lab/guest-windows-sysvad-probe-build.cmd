@echo off
setlocal
set job=C:\MixRelayLab\sysvad-probe-build-v1
if exist "%job%" exit /b 2
mkdir "%job%"
call :build > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:build
call C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 3
cd /d "%job%"
cl.exe /nologo /std:c++17 /EHsc /W4 /WX /O2 C:\Users\state303\SysvadAudioProbe.cpp /Fe:SysvadAudioProbe.exe ole32.lib uuid.lib
exit /b %errorlevel%
