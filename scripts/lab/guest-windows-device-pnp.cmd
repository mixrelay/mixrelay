@echo off
setlocal
set job=C:\ToneNariLab\windows-device-pnp-v7
if exist "%job%" exit /b 2
mkdir "%job%"
call :check > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:check
powershell.exe -NoProfile -NonInteractive -Command "if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719' -or (Get-Process -Id $PID).SessionId -eq 0) {exit 1}"
if errorlevel 1 exit /b 3
call C:\ToneNariLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
set CARGO_HOME=C:\ToneNariLab\cargo
set RUSTUP_HOME=C:\ToneNariLab\rustup
set PATH=%CARGO_HOME%\bin;%PATH%
set TONENARI_LAB_PNP_TEST=1
set TONENARI_LAB_PNP_SCOPE=RootLab
rem Explicit user approval: only this VM's LAB root may interrupt its default audio.
set TONENARI_LAB_ALLOW_DEFAULT_DRIVER_STOP=1
cd /d C:\ToneNariLab\source
cargo test --locked -p tonenari-engine-service --bin tonenari-engine-service --no-run
if errorlevel 1 exit /b 5
cargo clippy --locked -p tonenari-engine-service --all-targets -- -D warnings
if errorlevel 1 exit /b 6
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File scripts\lab\windows-lab-pnp-probe.ps1 -Evidence "%job%"
exit /b %errorlevel%
