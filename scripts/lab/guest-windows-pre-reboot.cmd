@echo off
setlocal
set job=C:\MixRelayLab\windows-pre-reboot-inventory-v1
if exist "%job%" exit /b 2
mkdir "%job%"
call :check > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:check
powershell.exe -NoProfile -NonInteractive -Command "if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719' -or (Get-Process -Id $PID).SessionId -eq 0) {exit 1}"
if errorlevel 1 exit /b 3
call C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
set CARGO_HOME=C:\MixRelayLab\cargo
set RUSTUP_HOME=C:\MixRelayLab\rustup
set PATH=%CARGO_HOME%\bin;%PATH%
set MIXRELAY_LAB_PNP_TEST=1
cd /d C:\MixRelayLab\source
cargo test --locked -p mixrelay-engine-service --bin mixrelay-engine-service native_wasapi_default_roles_read_only -- --ignored --nocapture --test-threads=1
exit /b %errorlevel%
