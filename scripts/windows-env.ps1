# Shared setup for the Windows build and run entry points.
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') {
    throw 'These scripts require Windows.'
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Install Rust stable with the MSVC toolchain from https://rustup.rs, then reopen PowerShell.'
}

# Import the developer environment so an ordinary PowerShell terminal also works.
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) {
        throw 'Install Visual Studio Build Tools with Desktop development with C++ and a Windows SDK.'
    }
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vsPath) {
        throw 'Install the Visual Studio Desktop development with C++ workload.'
    }
    Import-Module (Join-Path $vsPath 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')
    Enter-VsDevShell -VsInstallPath $vsPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
}

if (-not $env:GPUI_FXC_PATH) {
    $fxc = Get-Command fxc.exe -ErrorAction SilentlyContinue
    if ($fxc) {
        $env:GPUI_FXC_PATH = $fxc.Source
    } else {
        $sdkBin = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
        $fxcPath = Get-ChildItem -Path "$sdkBin/*/x64/fxc.exe" -ErrorAction SilentlyContinue |
            Sort-Object { [version]$_.Directory.Parent.Name } -Descending |
            Select-Object -First 1 -ExpandProperty FullName
        if (-not $fxcPath) {
            throw 'Windows SDK fxc.exe was not found. Install a Windows SDK or set GPUI_FXC_PATH to fxc.exe.'
        }
        $env:GPUI_FXC_PATH = $fxcPath
    }
}
if (-not (Test-Path -LiteralPath $env:GPUI_FXC_PATH -PathType Leaf)) {
    throw "GPUI_FXC_PATH does not point to a file: $env:GPUI_FXC_PATH"
}
