@echo off
setlocal
set job=C:\MixRelayLab\windows-device-recovery-v1
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
set MIXRELAY_LAB_AUDIO_TEST=1
cd /d C:\MixRelayLab\source
cargo test --locked -p mixrelay-engine-service --bin mixrelay-engine-service native_wasapi_device_input_reopen_and_remove -- --ignored --nocapture --test-threads=1
if errorlevel 1 exit /b 5
cargo test --locked -p mixrelay-engine-service --bin mixrelay-engine-service native_wasapi_output_recovers_with_new_clock_epoch -- --ignored --nocapture --test-threads=1
if errorlevel 1 exit /b 6
cargo test --locked -p mixrelay-engine-service --bin mixrelay-engine-service native_wasapi_graph_activation_and_removal -- --ignored --nocapture --test-threads=1
exit /b %errorlevel%
