@echo off
setlocal
set job=C:\ToneNariLab\windows-device-pnp-regression-v2
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
cd /d C:\ToneNariLab\source
cargo test --locked -p tonenari-audio-io -p tonenari-audio-graph -p tonenari-engine-service --lib --bins
if errorlevel 1 exit /b 5
cargo clippy --locked -p tonenari-engine-service --all-targets -- -D warnings
exit /b %errorlevel%
