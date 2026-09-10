@echo off
setlocal
rem VM-only artifact signing. Does not install or start the kernel driver.
set job=C:\MixRelayLab\signed-driver-v1
set build=windows-driver-v6
if "%~1"=="v7" (
    set job=C:\MixRelayLab\signed-driver-v2
    set build=windows-driver-v7
) else if "%~1"=="v8" (
    set job=C:\MixRelayLab\signed-driver-v3
    set build=windows-driver-v8
) else if not "%~1"=="" exit /b 2
if exist "%job%" exit /b 2
mkdir "%job%"
call :sign > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:sign
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized Windows VM'}; if (Confirm-SecureBootUEFI) {throw 'Secure Boot still enabled'}; Add-Type -Path C:\Users\state303\CodeIntegrityState.cs; $ci=[MixRelayCodeIntegrityState]::Read(); if (($ci -band 2) -eq 0) {throw 'Running kernel does not allow test signing; reboot first'}; if ((Get-Content C:\MixRelayLab\%build%\exit-code).Trim() -ne '0') {throw 'Native build did not pass'}; $cert=Get-Item Cert:\LocalMachine\My\BF2F641C417A264E7F51FC75A91E8AA19A87674D; if (-not $cert.HasPrivateKey -or $cert.NotAfter -le (Get-Date)) {throw 'Test certificate unusable'}; [pscustomobject]@{SecureBoot=$false; CodeIntegrityOptions=$ci; TestSigning=$true; BootTime=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime; Certificate=$cert.Thumbprint} | Format-List"
if errorlevel 1 exit /b 3
mkdir "%job%\package"
copy C:\MixRelayLab\%build%\MixRelayAudio.sys "%job%\package\MixRelayAudio.sys"
if errorlevel 1 exit /b 4
copy C:\MixRelayLab\%build%\MixRelayAudio.inf "%job%\package\MixRelayAudio.inf"
if errorlevel 1 exit /b 5
set signer=C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\arm64\signtool.exe
"%signer%" sign /v /fd SHA256 /sm /s My /sha1 BF2F641C417A264E7F51FC75A91E8AA19A87674D "%job%\package\MixRelayAudio.sys"
if errorlevel 1 exit /b 6
C:\MixRelayLab\wdk-arm64-26100\c\bin\10.0.26100.0\x86\Inf2Cat.exe /driver:"%job%\package" /os:10_GE_ARM64 /verbose
if errorlevel 1 exit /b 7
"%signer%" sign /v /fd SHA256 /sm /s My /sha1 BF2F641C417A264E7F51FC75A91E8AA19A87674D "%job%\package\MixRelayAudio.cat"
if errorlevel 1 exit /b 8
"%signer%" verify /v /pa "%job%\package\MixRelayAudio.sys"
if errorlevel 1 exit /b 9
"%signer%" verify /v /pa /c "%job%\package\MixRelayAudio.cat" "%job%\package\MixRelayAudio.sys"
if errorlevel 1 exit /b 10
powershell.exe -NoProfile -NonInteractive -Command "Get-FileHash %job%\package\* -Algorithm SHA256 | Format-List"
echo PASS: running test-signing kernel verified; SYS and catalog signed and verified. Driver NOT installed.
exit /b 0
