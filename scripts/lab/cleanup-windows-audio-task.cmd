@echo off
setlocal
rem Remove only this completed lab task; retain test logs and installed tools.
if exist C:\MixRelayLab\cleanup-audio-v3.log exit /b 2
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; $t=Get-ScheduledTask -TaskName 'MixRelayLabAudio' -ErrorAction SilentlyContinue; if ($t) {if ($t.State -eq 'Running' -or $t.Actions.Count -ne 1 -or $t.Actions[0].Arguments -ne '/d /c C:\Users\state303\audio-check.cmd') {throw 'Task running or ownership mismatch'}; Unregister-ScheduledTask -TaskName 'MixRelayLabAudio' -Confirm:$false}; if (Get-ScheduledTask -TaskName 'MixRelayLabAudio' -ErrorAction SilentlyContinue) {throw 'Task removal not confirmed'}; Write-Output 'PASS: owned task absent'" > C:\MixRelayLab\cleanup-audio-v3.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\cleanup-audio-v3.exit-code echo %result%
exit /b %result%
