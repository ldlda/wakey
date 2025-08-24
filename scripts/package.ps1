# Package a release tarball with binaries and init scripts.
# Usage: ./scripts/package.ps1 -Version 0.1.0 -OutDir dist
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$OutDir = "dist",
    [string[]]$Targets = @("armv7-unknown-linux-musleabihf")
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $MyInvocation.MyCommand.Path | Split-Path -Parent
$dist = Join-Path $root $OutDir
New-Item -ItemType Directory -Force -Path $dist | Out-Null

$pkgName = "wakey-$Version"
$pkgDir = Join-Path $dist $pkgName
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $pkgDir
New-Item -ItemType Directory -Force -Path $pkgDir | Out-Null

# Copy LICENSE/README if present
if (Test-Path "$root/README.md") { Copy-Item "$root/README.md" $pkgDir }
if (Test-Path "$root/LICENSE") { Copy-Item "$root/LICENSE" $pkgDir }

# Binaries per target
foreach ($t in $Targets) {
    $binName = if ($t -like "*-windows-*") { "wakey.exe" } else { "wakey" }
    $src = Join-Path $root "target/$t/release/$binName"
    if (-not (Test-Path $src)) { throw "Missing binary: $src" }
    $tDir = Join-Path $pkgDir "bin/$t"
    New-Item -ItemType Directory -Force -Path $tDir | Out-Null
    Copy-Item $src $tDir
}

# Init scripts
$initDir = Join-Path $pkgDir "init"
New-Item -ItemType Directory -Force -Path $initDir | Out-Null
Copy-Item "$root/scripts/init" $initDir -Recurse

# Systemd service (for Linux hosts)
Copy-Item "$root/scripts/systemd" $initDir -Recurse -ErrorAction SilentlyContinue

# Tarball
Push-Location $dist
if (Get-Command tar -ErrorAction SilentlyContinue) {
    tar -czf "$pkgName.tgz" "$pkgName"
}
else {
    # Fallback to zip on Windows
    Compress-Archive -Path "$pkgName" -DestinationPath "$pkgName.zip" -Force
}
Pop-Location

Write-Host "Packaged: $dist/$pkgName.(tgz|zip)"
