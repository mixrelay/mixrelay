param([Parameter(Mandatory=$true)][string]$Evidence)
$ErrorActionPreference = 'Stop'
if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719' -or
    (Get-Process -Id $PID).SessionId -eq 0) { throw 'Authorized interactive VM required' }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'PnP acceptance requires the authorized VM administrator session'
}
$original = @(Get-PnpDevice -Class AudioEndpoint | Where-Object FriendlyName -Like '*MixRelay SysVAD Bridge LAB*')
$original | Select-Object Status,FriendlyName,InstanceId | ConvertTo-Json | Set-Content -Encoding utf8 "$Evidence\before.json"
if ($original.Count -eq 0 -or @($original | Where-Object Status -Ne 'OK').Count -gt 0) {
    throw 'All LAB endpoints must be healthy before acceptance; preserving pre-existing state'
}
$result = 1
try {
    & cmd.exe /d /c ('cargo test --locked -p mixrelay-engine-service --bin mixrelay-engine-service native_wasapi_default_roles_read_only -- --ignored --nocapture --test-threads=1 > "' + $Evidence + '\defaults-before.log" 2>&1')
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read initial default endpoints' }
    & cargo test --locked -p mixrelay-engine-service --bin mixrelay-engine-service _pnp_recovery_and_offline_removal -- --ignored --nocapture --test-threads=1
    $result = $LASTEXITCODE
} finally {
    # A test panic executes its RAII guard. This independent boundary also
    # restores after the test process aborts; never touch non-LAB device nodes.
    if ($env:MIXRELAY_LAB_PNP_SCOPE -eq 'RootLab') {
        $root = Get-PnpDevice -InstanceId 'ROOT\MEDIA\0002'
        $flags = (Get-PnpDeviceProperty -InstanceId $root.InstanceId -KeyName DEVPKEY_Device_ConfigFlags).Data
        if ($root.Status -ne 'OK' -or ($flags -band 1) -ne 0) {
            $uid = $original[0].InstanceId.Substring('SWD\MMDEVAPI\'.Length)
            & "$PSScriptRoot\windows-lab-endpoint-toggle.ps1" -Uid $uid -Action Enable -Scope RootLab
        }
        Start-Sleep -Seconds 3
    }
    foreach ($device in $original) {
        $current = Get-PnpDevice -InstanceId $device.InstanceId
        if ($current.Status -ne 'OK') {
            $uid = $device.InstanceId.Substring('SWD\MMDEVAPI\'.Length)
            & "$PSScriptRoot\windows-lab-endpoint-toggle.ps1" -Uid $uid -Action Enable
        }
    }
    $after = @(Get-PnpDevice -Class AudioEndpoint | Where-Object FriendlyName -Like '*MixRelay SysVAD Bridge LAB*')
    $after | Select-Object Status,FriendlyName,InstanceId | ConvertTo-Json | Set-Content -Encoding utf8 "$Evidence\after.json"
    $missing = @($original | Where-Object { $_.InstanceId -notin $after.InstanceId })
    if ($after.Count -ne $original.Count -or $missing.Count -ne 0 -or @($after | Where-Object Status -Ne 'OK').Count -gt 0) {
        throw 'LAB endpoint restoration failed'
    }
    & cmd.exe /d /c ('cargo test --locked -p mixrelay-engine-service --bin mixrelay-engine-service native_wasapi_default_roles_read_only -- --ignored --nocapture --test-threads=1 > "' + $Evidence + '\defaults-after.log" 2>&1')
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read restored default endpoints' }
}
exit $result
