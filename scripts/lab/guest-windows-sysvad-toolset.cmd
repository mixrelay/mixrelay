@echo off
setlocal
rem Add official WDK MSBuild integration to the already authorized VM toolchain.
rem No security-policy changes and no automatic restart.
if exist C:\MixRelayLab\sysvad-toolset-v1.log exit /b 2
call :install > C:\MixRelayLab\sysvad-toolset-v1.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\sysvad-toolset-v1.exit-code echo %result%
exit /b %result%
:install
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; $s=Get-AuthenticodeSignature C:\MixRelayLab\vs_buildtools.exe; if ($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {throw 'Untrusted installer'}"
if errorlevel 1 exit /b 3
start "" /wait C:\MixRelayLab\vs_buildtools.exe modify --quiet --wait --norestart --installPath C:\MixRelayLab\BuildTools --add Component.Microsoft.Windows.DriverKit.BuildTools --add Microsoft.VisualStudio.Component.VC.ATL.ARM64 --add Microsoft.VisualStudio.Component.VC.ATL.ARM64.Spectre --add Microsoft.VisualStudio.Component.VC.Runtimes.ARM64.Spectre
set installer_result=%errorlevel%
echo Installer exit: %installer_result%
if not "%installer_result%"=="0" if not "%installer_result%"=="3010" exit /b %installer_result%
"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -all -products * -requires Component.Microsoft.Windows.DriverKit.BuildTools -format json
exit /b %installer_result%
