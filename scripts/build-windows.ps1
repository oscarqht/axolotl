[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')]
    [string]$Profile = 'debug',
    [switch]$Smoke
)

. "$PSScriptRoot/windows-env.ps1"
Push-Location (Join-Path $PSScriptRoot '..')
try {
    $cargoArgs = @('build', '-p', 'axolotl', '--locked')
    if ($Profile -eq 'release') { $cargoArgs += '--release' }
    if ($Smoke) { $cargoArgs += @('--features', 'smoke') }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo build failed (exit $LASTEXITCODE)." }
} finally {
    Pop-Location
}
