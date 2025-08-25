# One-shot: build, package, and print artifact location (no cross required).
# Usage: ./scripts/release.ps1 -Version 0.1.0 -Targets armv7-unknown-linux-musleabihf
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string[]]$Targets = @("armv7-unknown-linux-musleabihf")
)

$ErrorActionPreference = 'Stop'

foreach ($t in $Targets) {
    rustup target add $t
    cargo build --release --target $t
}
./scripts/package.ps1 -Version $Version -Targets $Targets
