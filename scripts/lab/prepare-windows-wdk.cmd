@echo off
setlocal
rem Pinned, project-local WDK. This does not install a driver or change boot policy.
if exist C:\ToneNariLab\wdk-prepare-v1.log exit /b 2
call :prepare > C:\ToneNariLab\wdk-prepare-v1.log 2>&1
set result=%errorlevel%
>C:\ToneNariLab\wdk-prepare-v1.exit-code echo %result%
exit /b %result%
:prepare
curl.exe --fail --location --retry 2 --max-time 180 https://api.nuget.org/v3-flatcontainer/microsoft.windows.wdk.arm64/10.0.26100.6584/microsoft.windows.wdk.arm64.10.0.26100.6584.nupkg -o C:\ToneNariLab\wdk-arm64-26100.nupkg
if errorlevel 1 exit /b 3
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; $p='C:\ToneNariLab\wdk-arm64-26100.nupkg'; if ((Get-FileHash $p -Algorithm SHA256).Hash -ne 'E705B2A63EAB891DEF8F98087666F93E8F21DA8E3B5DEF81A624B83FEF5BDAE9') {throw 'WDK package hash mismatch'}; Add-Type -AssemblyName System.IO.Compression.FileSystem; [IO.Compression.ZipFile]::ExtractToDirectory($p,'C:\ToneNariLab\wdk-arm64-26100'); $exe='C:\ToneNariLab\wdk-arm64-26100\c\tools\10.0.26100.0\ARM64\infverif.exe'; $sig=Get-AuthenticodeSignature $exe; if ($sig.Status -ne 'Valid' -or $sig.SignerCertificate.Subject -notmatch 'Microsoft') {throw 'WDK tool signature invalid'}; $sig | Select-Object Status,Path; Write-Output 'PASS: pinned ARM64 WDK extracted and Microsoft tool signature verified'"
if errorlevel 1 exit /b 4
exit /b 0
