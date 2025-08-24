# Publish to registry and tag
# Usage: ./scripts/publish.ps1 -Version 0.1.0 -Tag
param(
    [string]$Version,
    [switch]$Tag,
    [switch]$Publish
)

$ErrorActionPreference = 'Stop'

if ($Version) {
    $pattern = '(?m)^version\s*=\s*"[^"]+"'
    $replacement = ('version = "{0}"' -f $Version)
    (Get-Content Cargo.toml -Raw) -replace $pattern, $replacement | Set-Content Cargo.toml -Encoding UTF8
}

# Ensure build ok
cargo build --release

# Publish crate (only if -Publish and registry auth is available)
if ($Publish) {
    $token = $env:CARGO_REGISTRIES_CRATES_IO_TOKEN
    if (-not $token) { $token = $env:CARGO_REGISTRY_TOKEN }
    if (-not $token) {
        Write-Warning "No registry token found (CARGO_REGISTRIES_CRATES_IO_TOKEN or CARGO_REGISTRY_TOKEN). Skipping cargo publish."
    }
    else {
        try {
            cargo publish
        }
        catch {
            Write-Warning ("cargo publish failed: {0}" -f $_)
        }
    }
}

if ($Tag -and $Version) {
    git tag -f "v$Version"
    git push -f origin "v$Version"
}
