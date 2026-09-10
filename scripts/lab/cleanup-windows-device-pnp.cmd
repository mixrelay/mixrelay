@echo off
setlocal
set job=C:\MixRelayLab\windows-device-pnp-cleanup-v1
if exist "%job%" exit /b 2
mkdir "%job%"
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not authorized VM'}; foreach($name in @('MixRelayLabDevicePnpV1','MixRelayLabDevicePnpV2','MixRelayLabDevicePnpV3')) {$task=Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue; if (!$task) {continue}; if($task.State -eq 'Running' -or $task.Actions.Count -ne 1 -or $task.Actions[0].Execute -ine 'C:\Windows\System32\cmd.exe' -or $task.Actions[0].Arguments -ine '/d /c C:\Users\state303\device-pnp.cmd') {throw 'Task ownership or state mismatch'}; Unregister-ScheduledTask -TaskName $name -Confirm:$false; Write-Output ('Removed owned completed task '+$name)}; Get-PnpDevice -InstanceId 'ROOT\MEDIA\0002' | Select-Object Status,FriendlyName,InstanceId | ConvertTo-Json; Get-PnpDevice -Class AudioEndpoint | Where-Object FriendlyName -Like '*MixRelay SysVAD Bridge LAB*' | Select-Object Status,FriendlyName,InstanceId | ConvertTo-Json" > "%job%\output.log" 2>&1
set result=%errorlevel%
>"%job%\exit-code" echo %result%
exit /b %result%
