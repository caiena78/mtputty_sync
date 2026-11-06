# Builds release and assembles .\dist with the exe, config.toml and README side by side.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$exe = Join-Path $PSScriptRoot 'target\release\mtputty_sync.exe'
if (-not (Test-Path $exe)) { throw "exe not found: $exe" }

$dist = Join-Path $PSScriptRoot 'dist'
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
New-Item -ItemType Directory $dist | Out-Null

Copy-Item $exe, 'config.toml', 'README.md' -Destination $dist
Write-Host "Deployment folder ready: $dist"
Get-ChildItem $dist | Select-Object Name, Length
