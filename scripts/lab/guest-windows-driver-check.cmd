@echo off
setlocal
rem Build only: never installs a driver, certificate, or boot policy.
if "%~1"=="" exit /b 2
set job=C:\MixRelayLab\%~1
if exist "%job%" exit /b 2
mkdir "%job%"
call :check > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:check
call C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 3
set wdk=C:\MixRelayLab\wdk-arm64-26100\c
set INCLUDE=%wdk%\Include\10.0.26100.0\km;%wdk%\Include\10.0.26100.0\km\crt;%INCLUDE%
set LIB=%wdk%\Lib\10.0.26100.0\km\ARM64;%LIB%
mkdir "%job%\source"
tar.exe -xzf C:\MixRelayLab\driver-source.tar.gz -C "%job%\source"
if errorlevel 1 exit /b 4
cd /d "%job%"
cl.exe /nologo /c /kernel /std:c++17 /GR- /EHs-c- /W4 /WX /Zl /GS /guard:cf /D_ARM64_ /DNTDDI_VERSION=0x0A000008 /D_WIN32_WINNT=0x0A00 /I source\include source\src\MrWave.cpp source\src\MrTransport.cpp
if errorlevel 1 exit /b 5
link.exe /nologo /driver /subsystem:native,10.00 /entry:GsDriverEntry /machine:arm64 /nodefaultlib /dynamicbase /nxcompat /integritycheck /guard:cf /out:MixRelayAudio.sys MrWave.obj MrTransport.obj ntoskrnl.lib hal.lib portcls.lib stdunk.lib libcntpr.lib wdmsec.lib bufferoverflowfastfailk.lib
if errorlevel 1 exit /b 6
dumpbin.exe /headers MixRelayAudio.sys
if errorlevel 1 exit /b 7
copy source\MixRelayAudio.inf MixRelayAudio.inf
if errorlevel 1 exit /b 8
"%wdk%\tools\10.0.26100.0\ARM64\infverif.exe" /w /v MixRelayAudio.inf
if errorlevel 1 exit /b 9
rem The same bounded-copy code used in the DPC is tested natively in user mode.
set INCLUDE=%WindowsSdkDir%Include\%WindowsSDKVersion%ucrt;%WindowsSdkDir%Include\%WindowsSDKVersion%shared;%WindowsSdkDir%Include\%WindowsSDKVersion%um;%VCToolsInstallDir%include
set LIB=%VCToolsInstallDir%lib\arm64;%WindowsSdkDir%Lib\%WindowsSDKVersion%ucrt\arm64;%WindowsSdkDir%Lib\%WindowsSDKVersion%um\arm64
cl.exe /nologo /std:c++17 /EHsc /W4 /WX /O2 /I source\include source\tests\ring_test.cpp /Fe:ring-test.exe
if errorlevel 1 exit /b 10
ring-test.exe
if errorlevel 1 exit /b 11
exit /b 0
