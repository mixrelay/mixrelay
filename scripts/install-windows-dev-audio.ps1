param(
    [Parameter(Mandatory = $true)]
    [string]$DriverInf
)

$ErrorActionPreference = "Stop"

if (-not $IsWindows) {
    throw "ToneNari Windows audio installation only runs on Windows"
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this script from an elevated PowerShell session"
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$resolvedInf = (Resolve-Path $DriverInf).Path
if ([IO.Path]::GetExtension($resolvedInf) -ne ".inf") {
    throw "DriverInf must point to a signed ToneNari INF"
}

cargo build --manifest-path "$repoRoot\Cargo.toml" -p tonenari-engine-service --release
if ($LASTEXITCODE -ne 0) { throw "Engine build failed" }

$installRoot = Join-Path $env:ProgramFiles "ToneNari"
$engineTarget = Join-Path $installRoot "tonenari-engine-service.exe"
$engineSource = Join-Path $repoRoot "target\release\tonenari-engine-service.exe"
$timestamp = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssZ")
New-Item -ItemType Directory -Force -Path $installRoot | Out-Null
if (Test-Path $engineTarget) {
    Move-Item $engineTarget "$engineTarget.backup-$timestamp"
}
Copy-Item $engineSource $engineTarget

& pnputil.exe /add-driver $resolvedInf /install
if ($LASTEXITCODE -ne 0) { throw "Windows rejected the ToneNari driver package" }

# Audio clients and per-process loopback belong to the interactive user session. A logon task is
# therefore correct here; a Session 0 Windows service cannot own the user's WASAPI graph.
$taskName = "ToneNari Audio Engine"
& schtasks.exe /Create /F /SC ONLOGON /RL LIMITED /TN $taskName /TR ('"' + $engineTarget + '"')
if ($LASTEXITCODE -ne 0) { throw "Could not register the ToneNari logon task" }
& schtasks.exe /Run /TN $taskName
if ($LASTEXITCODE -ne 0) { throw "Could not start the ToneNari audio engine" }

Write-Host "Installed the signed ToneNari driver and per-user audio engine"
