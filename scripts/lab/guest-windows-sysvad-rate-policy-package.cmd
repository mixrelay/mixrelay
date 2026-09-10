@echo off
setlocal
rem Package/sign with the EXISTING VM certificate. No cert registration, boot
rem settings, driver-store writes, installation, default selection or PnP changes.
if "%~1"=="" exit /b 2
powershell.exe -NoProfile -NonInteractive -Command "if ('%~1' -notmatch '^v[0-9]+$') {exit 2}"
if not "%errorlevel%"=="0" exit /b 2
set build=C:\MixRelayLab\sysvad-rate-policy-build-%~1
set job=C:\MixRelayLab\sysvad-rate-policy-package-%~1
set source=%build%\source\audio\sysvad\TabletAudioSample\ARM64\Release
if exist "%job%" exit /b 2
mkdir "%job%"
call :package > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:package
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not authorized VM'}; if ((Get-Content ($env:build+'\exit-code')).Trim() -ne '0') {throw 'Build failed'}; $m=Get-Content ($env:build+'\source\MIXRELAY-RATE-POLICY.json') -Raw | ConvertFrom-Json; if ($m.version -ne 1 -or $m.pcmTransportVersion -ne 2 -or $m.qualified) {throw 'Unexpected source contract'}; $c=Get-Item Cert:\LocalMachine\My\BF2F641C417A264E7F51FC75A91E8AA19A87674D; if (!$c.HasPrivateKey -or $c.NotAfter -le (Get-Date)) {throw 'Existing certificate unavailable'}; $inf=Get-Content ($env:source+'\ComponentizedAudioSample.inf') -Raw; if (!$inf.Contains('Root\mixrelay_SysvadBridge') -or $inf.Contains('Root\sysvad_ComponentizedAudioSample')) {throw 'INF identity mismatch'}; $b=[IO.File]::ReadAllBytes($env:source+'\TabletAudioSample.sys'); $pe=[BitConverter]::ToInt32($b,60); if ([BitConverter]::ToUInt16($b,$pe+4) -ne 0xaa64 -or ([BitConverter]::ToUInt16($b,$pe+94) -band 128) -eq 0) {throw 'Expected ARM64 FORCE_INTEGRITY image'}"
if not "%errorlevel%"=="0" exit /b 3
mkdir "%job%\package"
copy "%source%\TabletAudioSample.sys" "%job%\package\TabletAudioSample.sys"
if not "%errorlevel%"=="0" exit /b 4
copy "%source%\ComponentizedAudioSample.inf" "%job%\package\ComponentizedAudioSample.inf"
if not "%errorlevel%"=="0" exit /b 5
copy C:\MixRelayLab\sysvad-package-v2\package\KeywordDetectorContosoAdapter.dll "%job%\package\KeywordDetectorContosoAdapter.dll"
if not "%errorlevel%"=="0" exit /b 6
set signer=C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\arm64\signtool.exe
"%signer%" sign /v /fd SHA256 /sm /s My /sha1 BF2F641C417A264E7F51FC75A91E8AA19A87674D "%job%\package\TabletAudioSample.sys"
if not "%errorlevel%"=="0" exit /b 7
C:\MixRelayLab\wdk-arm64-26100\c\tools\10.0.26100.0\ARM64\infverif.exe /u /v "%job%\package\ComponentizedAudioSample.inf"
if not "%errorlevel%"=="0" exit /b 8
rem StampInf uses the guest local date; keep catalog validation in that same
rem date domain. Do not mutate the clock or backdate the signed descriptor.
C:\MixRelayLab\wdk-arm64-26100\c\bin\10.0.26100.0\x86\Inf2Cat.exe /driver:"%job%\package" /os:10_GE_ARM64 /uselocaltime /verbose
if not "%errorlevel%"=="0" exit /b 9
"%signer%" sign /v /fd SHA256 /sm /s My /sha1 BF2F641C417A264E7F51FC75A91E8AA19A87674D "%job%\package\sysvad.cat"
if not "%errorlevel%"=="0" exit /b 10
"%signer%" verify /v /pa /c "%job%\package\sysvad.cat" "%job%\package\TabletAudioSample.sys" "%job%\package\KeywordDetectorContosoAdapter.dll"
if not "%errorlevel%"=="0" exit /b 11
powershell.exe -NoProfile -NonInteractive -Command "Get-FileHash ($env:job+'\package\*') | Format-List"
exit /b %errorlevel%
