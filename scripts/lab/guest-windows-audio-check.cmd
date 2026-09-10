@echo off
setlocal
rem Launch from the VM desktop. QEMU guest-exec runs in Session 0 and is not
rem evidence of WASAPI behavior in the logged-in user's audio session.
if exist C:\MixRelayLab\windows-audio-v6 exit /b 2
mkdir C:\MixRelayLab\windows-audio-v6
call :check > C:\MixRelayLab\windows-audio-v6\output.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\windows-audio-v6\exit-code echo %result%
exit /b %result%
:check
whoami
powershell.exe -NoProfile -NonInteractive -Command "$s=(Get-Process -Id $PID).SessionId; Write-Output ('Interactive session: '+$s); if ($s -eq 0) {exit 1}"
if errorlevel 1 exit /b 3
call C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
set CARGO_HOME=C:\MixRelayLab\cargo
set RUSTUP_HOME=C:\MixRelayLab\rustup
set PATH=%CARGO_HOME%\bin;%PATH%
set MIXRELAY_LAB_AUDIO_TEST=1
set MIXRELAY_LAB_AUDIO_CYCLES=120
cd /d C:\MixRelayLab\source
cargo test --locked -p mixrelay-engine-service --bin mixrelay-engine-service native_wasapi_ -- --ignored --nocapture --test-threads=1
exit /b %errorlevel%
