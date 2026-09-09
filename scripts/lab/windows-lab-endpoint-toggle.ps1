param(
    [Parameter(Mandatory=$true)][string]$Uid,
    [Parameter(Mandatory=$true)][ValidateSet('Disable','Enable')][string]$Action,
    [ValidateSet('Endpoint','RootLab')][string]$Scope = 'Endpoint'
)
$ErrorActionPreference = 'Stop'
if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {
    throw 'Not the authorized Windows VM'
}
if ($Uid -notmatch '^\{0\.0\.[01]\.00000000\}\.\{[0-9A-Fa-f-]{36}\}$') {
    throw 'Not an exact MMDevice endpoint UID'
}
$instance = 'SWD\MMDEVAPI\' + $Uid
$device = @(Get-PnpDevice -InstanceId $instance)
if ($device.Count -ne 1 -or $device[0].Class -ne 'AudioEndpoint' -or
    $device[0].FriendlyName -notlike '*ToneNari SysVAD Bridge LAB*' -or
    $device[0].InstanceId -ine $instance) {
    throw 'Refusing a non-LAB or ambiguous PnP target'
}
if ($Scope -eq 'RootLab') {
    if ($env:TONENARI_LAB_ALLOW_DEFAULT_DRIVER_STOP -ne '1') {
        throw 'Additional explicit user authorization for LAB root interruption is required'
    }
    # Endpoint-only PnP disable does not invalidate this SysVAD audio client.
    # The fallback is locked to the read-verified LAB driver, never a wildcard.
    $instance = 'ROOT\MEDIA\0002'
    $device = @(Get-PnpDevice -InstanceId $instance)
    $hardware = @((Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_HardwareIds).Data)
    $service = (Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_Service).Data
    $provider = (Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_DriverProvider).Data
    $image = (Get-ItemProperty HKLM:\SYSTEM\CurrentControlSet\Services\tonenari_sysvadbridge).ImagePath
    if ($device.Count -ne 1 -or $device[0].Class -ne 'MEDIA' -or
        $device[0].FriendlyName -ne 'ToneNari SysVAD Bridge LAB' -or
        $hardware.Count -ne 1 -or $hardware[0] -ine 'Root\tonenari_SysvadBridge' -or
        $service -ne 'tonenari_sysvadbridge' -or $provider -ne 'TODO-Set-Provider' -or
        $image -ine '\SystemRoot\System32\DriverStore\FileRepository\componentizedaudiosample.inf_arm64_80a702dce9732521\tabletaudiosample.sys') {
        throw 'LAB root driver identity changed; re-inspect before any PnP operation'
    }
    $status = (Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_DevNodeStatus).Data
    if ($Action -eq 'Disable' -and ($status -band 0x100) -ne 0) {
        throw 'LAB devnode has DN_NEED_RESTART: reboot the authorized VM before PnP acceptance'
    }
    $flags = (Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_ConfigFlags).Data
    $problem = (Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_ProblemCode).Data
    if ($Action -eq 'Enable' -and $device[0].Status -eq 'OK' -and $problem -eq 0 -and ($flags -band 1) -eq 0) {
        # A rejected disable may have changed nothing. Report the independently
        # verified enabled state instead of attempting another pending operation.
        Write-Output 'Exact LAB root already enabled: Status OK, ProblemCode 0, ConfigFlags not disabled'
        exit 0
    }
}
# No default-selection API is called. The authorized root interruption may make
# Windows temporarily choose another default; the harness records every role.
if ($Action -eq 'Disable') {
    if ($device[0].Status -ne 'OK') { throw 'Refusing to disable a pre-existing unhealthy endpoint' }
    if ($Scope -eq 'RootLab') {
        & C:\Windows\System32\pnputil.exe /disable-device $instance
        if ($LASTEXITCODE -ne 0) { throw "Native PnP disable failed or requires reboot: $LASTEXITCODE" }
    } else {
        Disable-PnpDevice -InstanceId $instance -Confirm:$false
    }
} else {
    if ($Scope -eq 'RootLab') {
        & C:\Windows\System32\pnputil.exe /enable-device $instance
        if ($LASTEXITCODE -ne 0) {
            # A deferred disable can leave ConfigFlags disabled while Status
            # remains OK. The normal WMI enable path cancels that configuration
            # even when PnPUtil rejects another pending operation (VM v7 proof).
            Enable-PnpDevice -InstanceId $instance -Confirm:$false
        }
        $restoredFlags = (Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_ConfigFlags).Data
        $restoredProblem = (Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_ProblemCode).Data
        if ((Get-PnpDevice -InstanceId $instance).Status -ne 'OK' -or $restoredProblem -ne 0 -or ($restoredFlags -band 1) -ne 0) {
            throw 'LAB enable did not restore both current and next-boot enabled state'
        }
    } else {
        Enable-PnpDevice -InstanceId $instance -Confirm:$false
    }
}
Get-PnpDevice -InstanceId $instance | Select-Object Status,Class,FriendlyName,InstanceId | ConvertTo-Json -Compress
