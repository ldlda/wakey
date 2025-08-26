# Build a rootfs tarball suitable for `wget -O- ... | tar -xz -C /` on OpenWrt.
# It lays out files as absolute-root paths inside the archive:
#   root/.bin/wakey
#   etc/init.d/wakey
# Usage: ./scripts/package_rootfs.ps1 -Version 0.1.0 -Target armv7-unknown-linux-musleabihf -OutDir dist
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Target,
    [string]$OutDir = "dist",
    [switch]$NoBin
)

$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $MyInvocation.MyCommand.Path | Split-Path -Parent
$dist = Join-Path $root $OutDir
New-Item -ItemType Directory -Force -Path $dist | Out-Null

$binName = if ($Target -like "*-windows-*") { "wakey.exe" } else { "wakey" }
$binSrc = Join-Path $root "target/$Target/release/$binName"

$staging = Join-Path $dist ("rootfs-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $staging | Out-Null

# Lay out rootfs tree
$rootDir = Join-Path $staging "root/.bin"
$etcDir = Join-Path $staging "etc/init.d"
New-Item -ItemType Directory -Force -Path $rootDir | Out-Null
New-Item -ItemType Directory -Force -Path $etcDir | Out-Null

if (-not $NoBin) {
    if (Test-Path $binSrc) {
        Copy-Item $binSrc (Join-Path $rootDir "wakey") -Force
    }
    else {
        throw "Missing binary: $binSrc (build it first or pass -NoBin)"
    }
}
# Normalize kill script line endings and copy
$killSrc = Join-Path $root 'scripts/kill_wakey.sh'
if (Test-Path $killSrc) {
    $killContent = Get-Content -Raw -LiteralPath $killSrc
    $killContent = $killContent -replace "`r`n", "`n"
    Set-Content -NoNewline -LiteralPath (Join-Path $rootDir "kill_wakey.sh") -Value $killContent -Encoding UTF8
}

# Normalize remote deploy script and copy (optional helper)
$deploySrc = Join-Path $root 'scripts/remote_deploy_wakey.sh'
if (Test-Path $deploySrc) {
    $deployContent = Get-Content -Raw -LiteralPath $deploySrc
    $deployContent = $deployContent -replace "`r`n", "`n"
    Set-Content -NoNewline -LiteralPath (Join-Path $rootDir "remote_deploy_wakey.sh") -Value $deployContent -Encoding UTF8
}

# Copy all OpenWrt init scripts present in repo
Get-ChildItem (Join-Path $root 'scripts/init/openwrt') -File | ForEach-Object {
    $dest = Join-Path $etcDir $_.Name
    $content = Get-Content -Raw -LiteralPath $_.FullName
    # normalize to LF line endings
    $content = $content -replace "`r`n", "`n"
    Set-Content -NoNewline -LiteralPath $dest -Value $content -Encoding UTF8
}

# Copy helper scripts intended for /etc/ldlda_help
$helpSrc = Join-Path $root 'scripts/ldlda_help'
if (Test-Path $helpSrc) {
    $etcHelpDir = Join-Path $staging 'etc/ldlda_help'
    New-Item -ItemType Directory -Force -Path $etcHelpDir | Out-Null
    Get-ChildItem $helpSrc -File | ForEach-Object {
        $d = Join-Path $etcHelpDir $_.Name
        $c = Get-Content -Raw -LiteralPath $_.FullName
        $c = $c -replace "`r`n", "`n"
        Set-Content -NoNewline -LiteralPath $d -Value $c -Encoding UTF8
    }
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
if ($NoBin) {
    Write-Host "Note: -NoBin used, binary not included. Only scripts/configs were packaged." -ForegroundColor Yellow
}
Write-Host "Helper scripts: /etc/ldlda_help/* (e.g., update_wakey.sh, update_tailscale.sh). Mark executable if needed: chmod +x /etc/ldlda_help/*.sh"
Write-Host "Kill helper: /root/.bin/kill_wakey.sh (make it executable: chmod +x /root/.bin/kill_wakey.sh)"
<# if (Test-Path $deploySrc) { #> Write-Host "Deploy helper: /root/.bin/remote_deploy_wakey.sh (chmod +x /root/.bin/remote_deploy_wakey.sh)" # }
