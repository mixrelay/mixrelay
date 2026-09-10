@echo off
setlocal
set job=C:\MixRelayLab\windows-device-pnp-regression-v2
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
cd /d C:\MixRelayLab\source
cargo test --locked -p mixrelay-audio-io -p mixrelay-audio-graph -p mixrelay-engine-service --lib --bins
if errorlevel 1 exit /b 5
cargo clippy --locked -p mixrelay-engine-service --all-targets -- -D warnings
exit /b %errorlevel%
