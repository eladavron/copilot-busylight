<#
.SYNOPSIS
Builds and tests, then copies the setup exe to dist\ with the version in its name.
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

& (Join-Path $PSScriptRoot 'build.ps1') -Test | Out-Host

# The version lives under [workspace.package] in the root Cargo.toml.
$version = (Select-String -Path (Join-Path $PSScriptRoot 'Cargo.toml') -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value
$dist = Join-Path $PSScriptRoot 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$output = Join-Path $dist "copilot-busylight-setup-$version.exe"
Copy-Item (Join-Path $PSScriptRoot 'target\release\copilot-busylight-setup.exe') $output -Force
Get-Item $output | Select-Object Name, @{ n = 'Size (KB)'; e = { [math]::Round($_.Length / 1KB) } }
