param(
    [Parameter(Mandatory=$true)][string]$Manifest,
    [Parameter(Mandatory=$true)][string]$Output
)
$ErrorActionPreference = 'Stop'
if ((Get-CimInstance Win32_ComputerSystemProduct).UUID -ne '712A936E-9D91-4455-A8A9-A1F65EB60719') {
    throw 'Not the authorized Windows VM'
}
$result = @()
foreach ($line in Get-Content -LiteralPath $Manifest) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    if ($line -notmatch '^([0-9a-f]{64})  ([a-zA-Z0-9_./-]+)$') { throw 'Invalid source manifest' }
    $expected = $Matches[1]
    $relative = $Matches[2]
    if ($relative.StartsWith('/') -or $relative.Split('/') -contains '..') { throw 'Source path escapes workspace' }
    $actual = (Get-FileHash -LiteralPath (Join-Path 'C:\ToneNariLab\source' $relative) -Algorithm SHA256).Hash.ToLowerInvariant()
    $result += [PSCustomObject]@{path=$relative; sha256=$actual; matches=($actual -eq $expected)}
}
$result | ConvertTo-Json | Set-Content -LiteralPath $Output -Encoding utf8
if (@($result | Where-Object matches -eq $false).Count -ne 0) { throw 'Source snapshot differs from host manifest' }
Write-Output ('Matched source files: ' + $result.Count)
