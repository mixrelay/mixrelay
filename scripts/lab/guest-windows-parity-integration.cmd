@echo off
setlocal
rem Non-interactive source integration only. No ignored native tests, devices or defaults.
set job=C:\ToneNariLab\windows-parity-integration-v5
if exist "%job%" exit /b 2
mkdir "%job%"
call :check > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:check
powershell.exe -NoProfile -NonInteractive -Command "if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {exit 1}"
if errorlevel 1 exit /b 3
call C:\ToneNariLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
set CARGO_HOME=C:\ToneNariLab\cargo
set RUSTUP_HOME=C:\ToneNariLab\rustup
set PATH=%CARGO_HOME%\bin;%PATH%
set task_packages=-p tonenari-audio-io -p tonenari-audio-graph -p tonenari-audio-engine -p tonenari-control-protocol -p tonenari-control-server -p tonenari-processor-host -p tonenari-engine-service
cd /d C:\ToneNariLab\source
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\ToneNariLab\verify-windows-source-snapshot.ps1 -Manifest C:\ToneNariLab\v5-source-sha256.txt -Output C:\ToneNariLab\v5-guest-verified.json
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
