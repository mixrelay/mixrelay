@echo off
setlocal
rem Exact-VM one-shot task for the explicitly authorized LAB PnP interruption.
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized VM'}; $name='ToneNariLabDevicePnpV7'; if (Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue) {throw 'Refusing to replace an existing task'}; $principal=New-ScheduledTaskPrincipal -UserId ($env:COMPUTERNAME+'\state303') -LogonType Interactive -RunLevel Highest; $action=New-ScheduledTaskAction -Execute 'C:\Windows\System32\cmd.exe' -Argument '/d /c C:\Users\state303\device-pnp.cmd'; Register-ScheduledTask -TaskName $name -Action $action -Principal $principal | Out-Null; Start-ScheduledTask -TaskName $name"
exit /b %errorlevel%
