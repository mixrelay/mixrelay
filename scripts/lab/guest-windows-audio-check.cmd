@echo off
setlocal
rem Launch from the VM desktop. QEMU guest-exec runs in Session 0 and is not
rem evidence of WASAPI behavior in the logged-in user's audio session.
if exist C:\ToneNariLab\windows-audio-v6 exit /b 2
mkdir C:\ToneNariLab\windows-audio-v6
call :check > C:\ToneNariLab\windows-audio-v6\output.log 2>&1
set result=%errorlevel%
>C:\ToneNariLab\windows-audio-v6\exit-code echo %result%
exit /b %result%
:check
whoami
powershell.exe -NoProfile -NonInteractive -Command "$s=(Get-Process -Id $PID).SessionId; Write-Output ('Interactive session: '+$s); if ($s -eq 0) {exit 1}"
if errorlevel 1 exit /b 3
call C:\ToneNariLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
set CARGO_HOME=C:\ToneNariLab\cargo
set RUSTUP_HOME=C:\ToneNariLab\rustup
set PATH=%CARGO_HOME%\bin;%PATH%
set TONENARI_LAB_AUDIO_TEST=1
set TONENARI_LAB_AUDIO_CYCLES=120
cd /d C:\ToneNariLab\source
cargo test --locked -p tonenari-engine-service --bin tonenari-engine-service native_wasapi_ -- --ignored --nocapture --test-threads=1
exit /b %errorlevel%
