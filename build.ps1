<#
.SYNOPSIS
Builds copilot-busylight with the Rust GNU toolchain (no Visual Studio needed).

.DESCRIPTION
Runs the tests with -Test, builds the tray app and hook, then builds
copilot-busylight-setup.exe, which embeds them. All three end up in target\release.
#>
[CmdletBinding()]
param(
    [switch]$Test
)

$ErrorActionPreference = 'Stop'

$cargoBin = Join-Path $HOME '.cargo\bin'
if (-not (Get-Command cargo -ErrorAction SilentlyContinue) -and (Test-Path $cargoBin)) {
    $env:PATH = "$cargoBin;$env:PATH"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Rust not found. Install it from https://rustup.rs (see README).'
}

Push-Location $PSScriptRoot
try {
    if ($Test) {
        cargo test --release -p copilot-busylight
        if ($LASTEXITCODE) { throw 'Tests failed' }
    }
    cargo build --release -p copilot-busylight
    if ($LASTEXITCODE) { throw 'Build failed' }
    # Must come second: it embeds the exes built above.
    cargo build --release -p copilot-busylight-setup
    if ($LASTEXITCODE) { throw 'Installer build failed' }
    Get-Item target\release\copilot-busylight.exe, target\release\copilot-busylight-hook.exe, target\release\copilot-busylight-setup.exe |
        Select-Object Name, @{ n = 'Size (KB)'; e = { [math]::Round($_.Length / 1KB) } }
} finally {
    Pop-Location
}
