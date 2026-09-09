@echo off
setlocal
rem Package only the upstream base driver and its required keyword adapter.
rem No sample APO extension installation; no replacement of the ToneNari driver.
set job=C:\ToneNariLab\sysvad-package-v2
set source=C:\ToneNariLab\sysvad-build-v2\source\audio\sysvad
if exist "%job%" exit /b 2
mkdir "%job%"
call :package > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:package
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-Content C:\ToneNariLab\sysvad-build-v2\exit-code).Trim() -ne '0') {throw 'Native baseline build failed'}; Add-Type -Path C:\Users\state303\CodeIntegrityState.cs; if ((Confirm-SecureBootUEFI) -or (([ToneNariCodeIntegrityState]::Read() -band 2) -eq 0)) {throw 'Unexpected signing state'}; $c=Get-Item Cert:\LocalMachine\My\BF2F641C417A264E7F51FC75A91E8AA19A87674D; if (-not $c.HasPrivateKey -or $c.NotAfter -le (Get-Date)) {throw 'Test certificate unavailable'}"
if errorlevel 1 exit /b 3
call C:\ToneNariLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
msbuild.exe "%source%\KeywordDetectorAdapter\KeywordDetectorContosoAdapter.vcxproj" /m:2 /p:Configuration=Release /p:Platform=ARM64 /p:WindowsTargetPlatformVersion=10.0.26100.0 /p:SignMode=Off /v:minimal /bl:"%job%\keyword.binlog"
if errorlevel 1 exit /b 5
mkdir "%job%\package"
copy "%source%\TabletAudioSample\ARM64\Release\TabletAudioSample.sys" "%job%\package\TabletAudioSample.sys"
if errorlevel 1 exit /b 6
copy "%source%\TabletAudioSample\ARM64\Release\ComponentizedAudioSample.inf" "%job%\package\ComponentizedAudioSample.inf"
if errorlevel 1 exit /b 7
copy "%source%\KeywordDetectorAdapter\ARM64\Release\KeywordDetectorContosoAdapter.dll" "%job%\package\KeywordDetectorContosoAdapter.dll"
if errorlevel 1 exit /b 8
set signer=C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\arm64\signtool.exe
"%signer%" sign /v /fd SHA256 /sm /s My /sha1 BF2F641C417A264E7F51FC75A91E8AA19A87674D "%job%\package\TabletAudioSample.sys" "%job%\package\KeywordDetectorContosoAdapter.dll"
if errorlevel 1 exit /b 9
rem Match the upstream project's DriverTargetPlatform=Universal. The stricter
rem /w isolation check failed in v1 and is retained as an unmet PRODUCT gate.
rem This reference package is not claimed to satisfy Windows Driver isolation.
C:\ToneNariLab\wdk-arm64-26100\c\tools\10.0.26100.0\ARM64\infverif.exe /u /v "%job%\package\ComponentizedAudioSample.inf"
if errorlevel 1 exit /b 10
C:\ToneNariLab\wdk-arm64-26100\c\bin\10.0.26100.0\x86\Inf2Cat.exe /driver:"%job%\package" /os:10_GE_ARM64 /verbose
if errorlevel 1 exit /b 11
"%signer%" sign /v /fd SHA256 /sm /s My /sha1 BF2F641C417A264E7F51FC75A91E8AA19A87674D "%job%\package\sysvad.cat"
if errorlevel 1 exit /b 12
"%signer%" verify /v /pa /c "%job%\package\sysvad.cat" "%job%\package\TabletAudioSample.sys" "%job%\package\KeywordDetectorContosoAdapter.dll"
if errorlevel 1 exit /b 13
powershell.exe -NoProfile -NonInteractive -Command "Get-FileHash %job%\package\* -Algorithm SHA256 | Format-List"
echo PASS: upstream baseline packaged and test-signed. Not installed.
exit /b 0
