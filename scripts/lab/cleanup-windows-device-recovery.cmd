@echo off
setlocal
if exist C:\MixRelayLab\device-recovery-cleanup-v1.log exit /b 2
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if ((Get-Content C:\MixRelayLab\windows-device-recovery-v1\exit-code).Trim() -ne '0') {throw 'Native checks did not complete successfully'}; $name='MixRelayLabDeviceRecovery'; $task=Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue; if ($task) {if ($task.State -eq 'Running' -or $task.Actions.Count -ne 1 -or $task.Actions[0].Execute -ne 'C:\Windows\System32\cmd.exe' -or $task.Actions[0].Arguments -ne '/d /c C:\Users\state303\device-recovery.cmd') {throw 'Task active or ownership mismatch'}; Unregister-ScheduledTask -TaskName $name -Confirm:$false}; if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {throw 'Task removal failed'}; Write-Output 'PASS: completed owned device recovery task removed; driver and default devices unchanged'" > C:\MixRelayLab\device-recovery-cleanup-v1.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\device-recovery-cleanup-v1.exit-code echo %result%
exit /b %result%
