@echo off
setlocal
rem One-shot, non-elevated task in the already logged-in lab account. No saved
rem credentials, logon rights, execution-policy changes, or default-device edits.
powershell.exe -NoProfile -NonInteractive -Command "$name='MixRelayLabAudio'; if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {throw 'Refusing to replace an existing task'}; $principal=New-ScheduledTaskPrincipal -UserId ($env:COMPUTERNAME+'\state303') -LogonType Interactive -RunLevel Limited; $action=New-ScheduledTaskAction -Execute 'C:\Windows\System32\cmd.exe' -Argument '/d /c C:\Users\state303\audio-check.cmd'; Register-ScheduledTask -TaskName $name -Action $action -Principal $principal | Out-Null; Start-ScheduledTask -TaskName $name"
exit /b %errorlevel%
