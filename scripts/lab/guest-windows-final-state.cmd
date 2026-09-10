@echo off
setlocal
if exist C:\MixRelayLab\final-state-v3.log exit /b 2
rem Read-only final inventory: do not infer cleanup from a dispatch return code.
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; $os=Get-CimInstance Win32_OperatingSystem; Write-Output ('Windows '+$os.Version+' '+$os.OSArchitecture); $task=Get-ScheduledTask -TaskName 'MixRelayLabAudio' -ErrorAction SilentlyContinue; if ($task) {throw 'Audio test task still exists'}; Write-Output 'PASS: one-shot audio task removed'; $processes=@(Get-CimInstance Win32_Process | Where-Object {$_.Name -like 'mixrelay*' -and $_.ExecutablePath -like 'C:\MixRelayLab\*'}); if ($processes.Count) {$processes | Select-Object ProcessId,Name,ExecutablePath; throw 'Owned engine/test process still running'}; Write-Output 'PASS: no owned engine/test process remains'; Get-FileHash 'C:\MixRelayLab\source\target\debug\mixrelay-engine-service.exe' -Algorithm SHA256 | Format-List; foreach ($job in @('windows-native-v8','windows-audio-v6')) {$code=Get-Content ('C:\MixRelayLab\'+$job+'\exit-code'); if ($code.Trim() -ne '0') {throw ($job+' did not pass: '+$code)}; Write-Output ('PASS: '+$job)}" > C:\MixRelayLab\final-state-v3.log 2>&1
set result=%errorlevel%
>C:\MixRelayLab\final-state-v3.exit-code echo %result%
exit /b %result%
