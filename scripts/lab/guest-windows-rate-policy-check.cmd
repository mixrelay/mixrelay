@echo off
setlocal
rem Compile and portable tests only. Never installs/reloads devices or changes signing.
if "%~1"=="" exit /b 2
if "%~2"=="" exit /b 2
set job=C:\ToneNariLab\%~1
if exist "%job%" exit /b 2
mkdir "%job%"
set archive=%~2
call :check > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:check
powershell.exe -NoProfile -NonInteractive -Command "if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {exit 1}"
if errorlevel 1 exit /b 3
call C:\ToneNariLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
set wdk=C:\ToneNariLab\wdk-arm64-26100\c
set INCLUDE=%wdk%\Include\10.0.26100.0\km;%wdk%\Include\10.0.26100.0\km\crt;%INCLUDE%
set LIB=%wdk%\Lib\10.0.26100.0\km\ARM64;%LIB%
mkdir "%job%\source"
tar.exe -xzf "%archive%" -C "%job%\source"
if errorlevel 1 exit /b 5
cd /d "%job%"
cl.exe /nologo /c /kernel /std:c++17 /GR- /EHs-c- /W4 /WX /Zl /GS /guard:cf /D_ARM64_ /DNTDDI_VERSION=0x0A000008 /D_WIN32_WINNT=0x0A00 /I source\include source\src\TnWave.cpp source\src\TnTransport.cpp
if errorlevel 1 exit /b 6
link.exe /nologo /driver /subsystem:native,10.00 /entry:GsDriverEntry /machine:arm64 /nodefaultlib /dynamicbase /nxcompat /integritycheck /guard:cf /out:ToneNariAudio.sys TnWave.obj TnTransport.obj ntoskrnl.lib hal.lib portcls.lib stdunk.lib libcntpr.lib wdmsec.lib bufferoverflowfastfailk.lib
if errorlevel 1 exit /b 7
dumpbin.exe /headers ToneNariAudio.sys
if errorlevel 1 exit /b 8
set INCLUDE=%WindowsSdkDir%Include\%WindowsSDKVersion%ucrt;%WindowsSdkDir%Include\%WindowsSDKVersion%shared;%WindowsSdkDir%Include\%WindowsSDKVersion%um;%VCToolsInstallDir%include
set LIB=%VCToolsInstallDir%lib\arm64;%WindowsSdkDir%Lib\%WindowsSDKVersion%ucrt\arm64;%WindowsSdkDir%Lib\%WindowsSDKVersion%um\arm64
for %%T in (rate_policy ring pcm) do (
  cl.exe /nologo /std:c++17 /EHsc /W4 /WX /O2 /I source\include source\tests\%%T_test.cpp /Fe:%%T-test.exe
  if errorlevel 1 exit /b 9
  %%T-test.exe
  if errorlevel 1 exit /b 10
)
copy source\ToneNariAudio.inf ToneNariAudio.inf
"%wdk%\tools\10.0.26100.0\ARM64\infverif.exe" /w /v ToneNariAudio.inf
if errorlevel 1 exit /b 11
exit /b 0
