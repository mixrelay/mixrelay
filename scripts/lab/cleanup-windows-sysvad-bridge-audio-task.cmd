@echo off
setlocal
rem Remove only the completed interactive smoke task; preserve devices and evidence.
if exist C:\MixRelayLab\sysvad-bridge-audio-cleanup-v1.log exit /b 2
rem Task Scheduler may normalize the account name to a SID; compare resolved identity.
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; if (!(Test-Path 'C:\MixRelayLab\sysvad-bridge-audio-v11\exit-code')) {throw 'Smoke completion marker missing'}; $name='MixRelayLabSysvadBridgeAudio'; $t=Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue; if ($t) {$expected=([System.Security.Principal.NTAccount]::new($env:COMPUTERNAME,'state303')).Translate([System.Security.Principal.SecurityIdentifier]).Value; $owner=$t.Principal.UserId; if ($owner -notmatch '^S-1-') {$owner=([System.Security.Principal.NTAccount]::new($owner)).Translate([System.Security.Principal.SecurityIdentifier]).Value}; if ($t.State -eq 'Running' -or $t.Actions.Count -ne 1 -or $t.Actions[0].Execute -ne 'C:\Windows\System32\cmd.exe' -or $t.Actions[0].Arguments -ne '/d /c C:\Users\state303\sysvad-bridge-audio.cmd' -or $owner -ne $expected) {throw 'Task running or ownership mismatch'}; Unregister-ScheduledTask -TaskName $name -Confirm:$false}; if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {throw 'Task removal not confirmed'}; Write-Output 'PASS: owned SysVAD bridge audio task absent; sample driver and logs retained'" > C:\MixRelayLab\sysvad-bridge-audio-cleanup-v1.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\sysvad-bridge-audio-cleanup-v1.exit-code echo %result%
exit /b %result%
