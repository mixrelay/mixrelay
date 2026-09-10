@echo off
setlocal
if exist C:\MixRelayLab\sysvad-prepare-v1.log exit /b 2
call :prepare > C:\MixRelayLab\sysvad-prepare-v1.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\sysvad-prepare-v1.exit-code echo %result%
exit /b %result%
:prepare
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if (Test-Path C:\MixRelayLab\sdk-cpp-26100) {throw 'SDK destination already exists'}; if (Test-Path C:\MixRelayLab\sdk-arm64-26100) {throw 'SDK destination already exists'}"
if errorlevel 1 exit /b 3
curl.exe --fail --location --retry 2 https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp/10.0.26100.6584/microsoft.windows.sdk.cpp.10.0.26100.6584.nupkg -o C:\MixRelayLab\sdk-cpp-26100.zip
if errorlevel 1 exit /b 4
curl.exe --fail --location --retry 2 https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp.arm64/10.0.26100.6584/microsoft.windows.sdk.cpp.arm64.10.0.26100.6584.nupkg -o C:\MixRelayLab\sdk-arm64-26100.zip
if errorlevel 1 exit /b 5
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-FileHash C:\MixRelayLab\sdk-cpp-26100.zip).Hash -ne '5D31B38205BDD9AC761B4CB39FBBC6B7209B01C11194324AFC674D7D119483A0') {throw 'SDK hash mismatch'}; if ((Get-FileHash C:\MixRelayLab\sdk-arm64-26100.zip).Hash -ne 'AC06922186FDD75E4FF1033A304EA629D7917BD0CA85E483762F8109584CCFC4') {throw 'SDK ARM64 hash mismatch'}; Expand-Archive C:\MixRelayLab\sdk-cpp-26100.zip C:\MixRelayLab\sdk-cpp-26100; Expand-Archive C:\MixRelayLab\sdk-arm64-26100.zip C:\MixRelayLab\sdk-arm64-26100; Write-Output 'PASS: pinned SDK packages verified and extracted; no system toolchain replacement'"
exit /b %errorlevel%
