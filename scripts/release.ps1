# One-shot: build with cross, package, and print artifact location.
# Usage: ./scripts/release.ps1 -Version 0.1.0 -Targets armv7-unknown-linux-musleabihf
param(
  [Parameter(Mandatory=$true)][string]$Version,
  [string[]]$Targets = @("armv7-unknown-linux-musleabihf")
)

$ErrorActionPreference = 'Stop'

./scripts/cross_build.ps1 -Targets $Targets -Release
./scripts/package.ps1 -Version $Version -Targets $Targets
