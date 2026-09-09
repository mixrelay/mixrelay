@echo off
setlocal
rem Official, hash-pinned ARM64 binaries; all files stay inside the named lab.
if not exist C:\ToneNariLab mkdir C:\ToneNariLab
if exist C:\ToneNariLab\toolchains.exit-code exit /b 2
call :install > C:\ToneNariLab\toolchains.log 2>&1
set result=%errorlevel%
>C:\ToneNariLab\toolchains.exit-code echo %result%
exit /b %result%
:install
curl.exe --fail --location --retry 3 https://static.rust-lang.org/rustup/archive/1.28.2/aarch64-pc-windows-msvc/rustup-init.exe --output C:\ToneNariLab\rustup-init.exe
if errorlevel 1 exit /b 1
powershell.exe -NoProfile -NonInteractive -Command "if ((Get-FileHash 'C:\ToneNariLab\rustup-init.exe' -Algorithm SHA256).Hash -ne 'de9f7d29ccd39efa59a3dda3ec363b396e09b92681229b9b8f6aaa4c84285e9c') {exit 1}"
if errorlevel 1 exit /b 1
set CARGO_HOME=C:\ToneNariLab\cargo
set RUSTUP_HOME=C:\ToneNariLab\rustup
C:\ToneNariLab\rustup-init.exe -y --no-modify-path --profile minimal --default-host aarch64-pc-windows-msvc --default-toolchain 1.96.0
if errorlevel 1 exit /b 1
%CARGO_HOME%\bin\rustup.exe component add clippy rustfmt
if errorlevel 1 exit /b 1
curl.exe --fail --location --retry 3 https://nodejs.org/dist/v24.19.0/node-v24.19.0-win-arm64.zip --output C:\ToneNariLab\node.zip
if errorlevel 1 exit /b 1
powershell.exe -NoProfile -NonInteractive -Command "if ((Get-FileHash 'C:\ToneNariLab\node.zip' -Algorithm SHA256).Hash -ne '8502f4a50b458d4cc38ed8f2001556c2cd239d464920f74017926ccb1e1c157f') {exit 1}; Expand-Archive -LiteralPath 'C:\ToneNariLab\node.zip' -DestinationPath 'C:\ToneNariLab\node'"
if errorlevel 1 exit /b 1
%CARGO_HOME%\bin\rustc.exe --version --verbose
C:\ToneNariLab\node\node-v24.19.0-win-arm64\node.exe --version
