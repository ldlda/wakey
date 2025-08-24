# Publish to registry and tag
# Usage: ./scripts/publish.ps1 -Version 0.1.0 -Tag
# Publish to registry and tag (manual; CI does not use this)
# Usage:
#   ./scripts/publish.ps1 -Version 0.1.0 -Publish          # publish to your configured Cargo registry (crates.io or Gitea)
#   ./scripts/publish.ps1 -Version 0.1.0 -Tag              # create/push git tag v0.1.0
#   ./scripts/publish.ps1 -Version 0.1.0 -Publish -Tag     # both
#   ./scripts/publish.ps1 -Version 0.1.0 -Publish -Registry gitea   # publish to named registry
param(
    [string]$Version,
    [switch]$Tag,
    [switch]$Publish
    [switch]$ForceTag,
    [string]$Registry
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
# Publish crate (only if -Publish). Relies on your Cargo config/credentials.
if ($Publish) {
    try {
        $pubArgs = @('publish')
        if ($Registry) { $pubArgs += @('--registry', $Registry) }
        cargo @pubArgs
    }
    catch {
        Write-Warning ("cargo publish failed: {0}" -f $_)
    }
}

if ($Tag -and $Version) {
    git tag -f "v$Version"
    git push -f origin "v$Version"
}
