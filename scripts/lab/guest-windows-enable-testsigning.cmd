@echo off
setlocal
rem Explicitly authorized for this disposable VM only. Never run on another host.
if exist C:\ToneNariLab\enable-testsigning-v1.log exit /b 2
call :enable > C:\ToneNariLab\enable-testsigning-v1.log 2>&1
set result=%errorlevel%
>C:\ToneNariLab\enable-testsigning-v1.exit-code echo %result%
exit /b %result%
:enable
powershell.exe -NoProfile -NonInteractive -Command "$ErrorActionPreference='Stop'; if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {throw 'Not the authorized Windows VM'}; if (Confirm-SecureBootUEFI) {throw 'Secure Boot is still enabled'}; $drive=Get-CimInstance -Namespace root/CIMV2/Security/MicrosoftVolumeEncryption -ClassName Win32_EncryptableVolume -Filter 'DriveLetter=''C:'''; if ($drive.ProtectionStatus -ne 0) {throw 'BitLocker protection changed; stop before BCD changes'}; $name='CN=ToneNari Windows VM Test 20260907'; if (Get-ChildItem Cert:\LocalMachine\My | Where-Object Subject -eq $name) {throw 'Test certificate already exists; inspect instead of replacing'}; $cert=New-SelfSignedCertificate -Type CodeSigningCert -Subject $name -FriendlyName 'ToneNari isolated Windows VM test only' -CertStoreLocation Cert:\LocalMachine\My -KeyExportPolicy NonExportable -HashAlgorithm SHA256 -KeyLength 2048 -NotAfter (Get-Date).AddDays(30); Export-Certificate -Cert $cert -FilePath C:\ToneNariLab\ToneNari-VM-test.cer | Out-Null; foreach ($storeName in @('Root','TrustedPublisher')) { $store=New-Object System.Security.Cryptography.X509Certificates.X509Store($storeName,'LocalMachine'); try { $store.Open('ReadWrite'); $store.Add($cert) } finally { $store.Close() } }; $cert | Select-Object Subject,Thumbprint,NotAfter | Format-List; Write-Output 'PASS: VM identity, Secure Boot off, test-only non-exportable code-signing certificate registered'"
if errorlevel 1 exit /b 3
bcdedit.exe /set {current} testsigning on
if errorlevel 1 exit /b 4
bcdedit.exe /enum {current}
if errorlevel 1 exit /b 5
powershell.exe -NoProfile -NonInteractive -Command "Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Filter signtool.exe -Recurse | ForEach-Object FullName"
echo PASS: test-signing BCD configured; reboot and runtime verification still required
exit /b 0
