@echo off
setlocal
if exist C:\MixRelayLab\windows-recovery-final-checks-v1 exit /b 2
mkdir C:\MixRelayLab\windows-recovery-final-checks-v1
call :check > C:\MixRelayLab\windows-recovery-final-checks-v1\output.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\windows-recovery-final-checks-v1\exit-code echo %result%
exit /b %result%
:check
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\MixRelayLab\verify-windows-source-snapshot.ps1 -Manifest C:\MixRelayLab\v4-source-sha256.txt -Output C:\MixRelayLab\v4-guest-verified.json
if errorlevel 1 exit /b 3
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'VM mismatch'}; foreach($file in @('C:\MixRelayLab\source\scripts\lab\windows-lab-endpoint-toggle.ps1','C:\MixRelayLab\source\scripts\lab\windows-lab-pnp-probe.ps1','C:\MixRelayLab\windows-lab-pnp-restore.ps1')) {$tokens=$null; $errors=$null; [System.Management.Automation.Language.Parser]::ParseFile($file,[ref]$tokens,[ref]$errors) | Out-Null; if($errors.Count) {throw ($errors | Out-String)}}; Write-Output 'Three scripts: parser_errors=0'; $owned=@{'MixRelayLabDevicePnpV7'='/d /c C:\Users\state303\device-pnp.cmd'; 'MixRelayLabPnpRestoreV1'='/d /c C:\MixRelayLab\pnp-restore.cmd'; 'MixRelayLabPostRestoreInventoryV1'='/d /c C:\MixRelayLab\post-restore.cmd'}; foreach($name in $owned.Keys) {$task=Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue; if(!$task) {continue}; if($task.State -eq 'Running' -or $task.Actions.Count -ne 1 -or $task.Actions[0].Execute -ine 'C:\Windows\System32\cmd.exe' -or $task.Actions[0].Arguments -ine $owned[$name]) {throw 'Task ownership or state mismatch'}; Unregister-ScheduledTask -TaskName $name -Confirm:$false; Write-Output ('Removed owned completed task '+$name)}"
if errorlevel 1 exit /b 4
call C:\MixRelayLab\pnp-final-inspect.cmd
exit /b %errorlevel%
