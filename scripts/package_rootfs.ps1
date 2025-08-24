# Build a rootfs tarball suitable for `wget -O- ... | tar -xz -C /` on OpenWrt.
# It lays out files as absolute-root paths inside the archive:
#   root/.bin/wakey
#   etc/init.d/wakey
# Usage: ./scripts/package_rootfs.ps1 -Version 0.1.0 -Target armv7-unknown-linux-musleabihf -OutDir dist
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Target,
    [string]$OutDir = "dist"
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $MyInvocation.MyCommand.Path | Split-Path -Parent
$dist = Join-Path $root $OutDir
New-Item -ItemType Directory -Force -Path $dist | Out-Null

$binName = if ($Target -like "*-windows-*") { "wakey.exe" } else { "wakey" }
$binSrc = Join-Path $root "target/$Target/release/$binName"
if (-not (Test-Path $binSrc)) { throw "Missing binary: $binSrc (build it first)" }

$staging = Join-Path $dist ("rootfs-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $staging | Out-Null

# Lay out rootfs tree
$rootDir = Join-Path $staging "root/.bin"
$etcDir = Join-Path $staging "etc/init.d"
New-Item -ItemType Directory -Force -Path $rootDir | Out-Null
New-Item -ItemType Directory -Force -Path $etcDir | Out-Null

Copy-Item $binSrc (Join-Path $rootDir "wakey") -Force

# Copy all OpenWrt init scripts present in repo
Get-ChildItem (Join-Path $root 'scripts/init/openwrt') -File | ForEach-Object {
    $dest = Join-Path $etcDir $_.Name
    Copy-Item $_.FullName $dest -Force
}

# Create tarball
$pkgName = "wakey-rootfs-$Version-$Target.tgz"
Push-Location $staging
if (Get-Command tar -ErrorAction SilentlyContinue) {
    tar -czf (Join-Path $dist $pkgName) *
}
else {
    # Fallback to zip (router can unzip too if available); prefer tar if possible
    Compress-Archive -Path * -DestinationPath (Join-Path $dist ($pkgName -replace '\.tgz$', '.zip')) -Force
}
Pop-Location

# Cleanup staging
Remove-Item -Recurse -Force $staging

Write-Host "Rootfs package: " (Join-Path $dist $pkgName)
Write-Host "On router: wget -O- <URL/$pkgName> | tar -xz -C /"
Write-Host "Then: chmod +x /etc/init.d/wakey && /etc/init.d/wakey enable && /etc/init.d/wakey start"
Write-Host "For any additional init scripts (e.g., update_tailscale), also chmod +x and enable/start as needed."
