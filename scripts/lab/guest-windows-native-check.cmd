@echo off
setlocal
rem Native ARM64 build in the isolated lab, never the user's installed application.
if "%~1"=="" exit /b 2
set job=C:\MixRelayLab\%~1
if exist "%job%" exit /b 2
mkdir "%job%"
call :check > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
:check
"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe" -all -products * -format json
if not exist C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat exit /b 3
call C:\MixRelayLab\BuildTools\VC\Auxiliary\Build\vcvarsall.bat arm64
if errorlevel 1 exit /b 4
set CARGO_HOME=C:\MixRelayLab\cargo
set RUSTUP_HOME=C:\MixRelayLab\rustup
set PATH=%CARGO_HOME%\bin;C:\MixRelayLab\node\node-v24.19.0-win-arm64;%PATH%
rustc --version --verbose
if errorlevel 1 exit /b 5
where cl
where link
node --version
if not exist C:\MixRelayLab\source mkdir C:\MixRelayLab\source
tar.exe -xzf C:\MixRelayLab\source-20260906.tar.gz -C C:\MixRelayLab\source
if errorlevel 1 exit /b 6
cd /d C:\MixRelayLab\source
cargo build --locked -p mixrelay-engine-service
if errorlevel 1 exit /b 7
cargo test --locked -p mixrelay-audio-io -p mixrelay-audio-engine -p mixrelay-engine-service --lib --bins
if errorlevel 1 exit /b 8
cargo clippy --locked -p mixrelay-audio-io -p mixrelay-audio-engine -p mixrelay-engine-service --lib --bins --tests -- -D warnings
if errorlevel 1 exit /b 9
exit /b 0
