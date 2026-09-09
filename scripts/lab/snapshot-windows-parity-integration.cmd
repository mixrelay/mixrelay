@echo off
setlocal
rem Read-only snapshot of an active log; guest-agent file pull cannot read a writer-locked log.
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'VM mismatch'}; Get-Content -LiteralPath 'C:\ToneNariLab\windows-parity-integration-v5\output.log' | Set-Content -LiteralPath 'C:\ToneNariLab\windows-parity-integration-v5\snapshot.log'"
exit /b %errorlevel%
