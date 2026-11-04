[CmdletBinding()]
param(
    [ValidateSet('debug', 'release')]
    [string]$Profile = 'debug',
    [switch]$SmokeTest
)

. "$PSScriptRoot/windows-env.ps1"
Push-Location (Join-Path $PSScriptRoot '..')
try {
    $cargoArgs = @('run', '-p', 'axolotl', '--locked')
    if ($Profile -eq 'release') { $cargoArgs += '--release' }
    if ($SmokeTest) { $cargoArgs += @('--features', 'smoke', '--', '--smoke-test') }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Axolotl failed (exit $LASTEXITCODE)." }
} finally {
    Pop-Location
}
