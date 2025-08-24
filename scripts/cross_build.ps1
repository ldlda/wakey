# Build release binaries using cross for Linux targets.
# Requires: cross installed (cargo install cross)
param(
    [string[]]$Targets = @("armv7-unknown-linux-musleabihf"),
    [switch]$Release
)

$ErrorActionPreference = 'Stop'

$mode = if ($Release) { "--release" } else { "" }

foreach ($t in $Targets) {
    Write-Host "Building for $t..."
    cross build $mode --target $t
}

Write-Host "Done."
