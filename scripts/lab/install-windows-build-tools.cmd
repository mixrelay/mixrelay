@echo off
setlocal
rem Run only after the user has accepted Microsoft's Build Tools license.
rem Uses the official release channel; no policy/security bypass or reboot.
if not exist C:\ToneNariLab mkdir C:\ToneNariLab
if exist C:\ToneNariLab\build-tools-install.exit-code exit /b 2
call :install > C:\ToneNariLab\build-tools-install.log 2>&1
set result=%errorlevel%
>C:\ToneNariLab\build-tools-install.exit-code echo %result%
exit /b %result%
:install
curl.exe --fail --location --retry 3 https://aka.ms/vs/17/release/vs_buildtools.exe --output C:\ToneNariLab\vs_buildtools.exe
if errorlevel 1 exit /b 1
powershell.exe -NoProfile -NonInteractive -Command "$s=Get-AuthenticodeSignature 'C:\ToneNariLab\vs_buildtools.exe'; $s | Format-List Status,SignerCertificate; if ($s.Status -ne 'Valid' -or $s.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {exit 1}"
if errorlevel 1 exit /b 1
start "" /wait C:\ToneNariLab\vs_buildtools.exe --quiet --wait --norestart --installPath C:\ToneNariLab\BuildTools --add Microsoft.VisualStudio.Workload.VCTools --add Microsoft.VisualStudio.Component.VC.Tools.ARM64 --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.26100 --includeRecommended
set installer_result=%errorlevel%
echo Installer exit: %installer_result%
if not "%installer_result%"=="0" if not "%installer_result%"=="3010" exit /b %installer_result%
"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -all -products * -format json
exit /b %installer_result%
