$ErrorActionPreference = 'Stop'
if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') { throw 'VM mismatch' }
$instance = 'ROOT\MEDIA\0002'
$device = Get-PnpDevice -InstanceId $instance
$hardware = @((Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_HardwareIds).Data)
$service = (Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_Service).Data
$provider = (Get-PnpDeviceProperty -InstanceId $instance -KeyName DEVPKEY_Device_DriverProvider).Data
$image = (Get-ItemProperty HKLM:\SYSTEM\CurrentControlSet\Services\mixrelay_sysvadbridge).ImagePath
if ($device.Class -ne 'MEDIA' -or $device.FriendlyName -ne 'MixRelay SysVAD Bridge LAB' -or $hardware.Count -ne 1 -or $hardware[0] -ine 'Root\mixrelay_SysvadBridge' -or $service -ne 'mixrelay_sysvadbridge' -or $provider -ne 'TODO-Set-Provider' -or $image -ine '\SystemRoot\System32\DriverStore\FileRepository\componentizedaudiosample.inf_arm64_80a702dce9732521\tabletaudiosample.sys') { throw 'LAB identity mismatch' }
# Normal enable only: undo the disabled configuration requested by the probe.
# No registry edits, restart, reinstall, or other device target is allowed here.
Enable-PnpDevice -InstanceId $instance -Confirm:$false
