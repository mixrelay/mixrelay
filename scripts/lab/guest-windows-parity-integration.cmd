@echo off
setlocal
rem Non-interactive source integration only. No ignored native tests, devices or defaults.
set job=C:\MixRelayLab\windows-parity-integration-v5
if exist "%job%" exit /b 2
mkdir "%job%"
call :check > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:check
powershell.exe -NoProfile -NonInteractive -Command "if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {exit 1}"
if errorlevel 1 exit /b 3
call C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
set CARGO_HOME=C:\MixRelayLab\cargo
set RUSTUP_HOME=C:\MixRelayLab\rustup
set PATH=%CARGO_HOME%\bin;%PATH%
set task_packages=-p mixrelay-audio-io -p mixrelay-audio-graph -p mixrelay-audio-engine -p mixrelay-control-protocol -p mixrelay-control-server -p mixrelay-processor-host -p mixrelay-engine-service
cd /d C:\MixRelayLab\source
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\MixRelayLab\verify-windows-source-snapshot.ps1 -Manifest C:\MixRelayLab\v5-source-sha256.txt -Output C:\MixRelayLab\v5-guest-verified.json
if errorlevel 1 exit /b 9
rustc -vV
if errorlevel 1 exit /b 5
cargo --version
if errorlevel 1 exit /b 6
cargo build --locked %task_packages% --lib --bins
if errorlevel 1 exit /b 7
cargo test --locked %task_packages% --lib --bins
if errorlevel 1 exit /b 8
cargo clippy --locked %task_packages% --all-targets -- -D warnings
exit /b %errorlevel%
