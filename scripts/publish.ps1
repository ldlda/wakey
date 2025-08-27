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
    [switch]$Publish,
    [string]$Registry
)

$ErrorActionPreference = 'Stop'

# If no version provided, deduce from Cargo.toml
if (-not $Version) {
    $cargoToml = Get-Content Cargo.toml -Raw
    if ($cargoToml -match 'version\s*=\s*"([^"]+)"') {
        $Version = $matches[1]
        Write-Host "Deduced version from Cargo.toml: $Version"
    }
    else {
        Write-Error "Could not deduce version from Cargo.toml. Please specify -Version."
        exit 1
    }
}

if ($Version) {
    $pattern = '(?m)^version\s*=\s*"[^"]+"'
    $replacement = ('version = "{0}"' -f $Version)
    (Get-Content Cargo.toml -Raw) -replace $pattern, $replacement | Set-Content Cargo.toml -Encoding UTF8 -NoNewline
}

# Ensure build ok
cargo fmt
cargo build --release

# Publish crate (only if -Publish and registry auth is available)
# Publish crate (only if -Publish). Relies on your Cargo config/credentials.
if ($Publish) {
    try {
        $pubArgs = @('publish')
        if ($Registry) { $pubArgs += @('--registry', $Registry) }
        cargo @pubArgs

        Write-Host ("published. ran cargo {0}" -f ($pubArgs -join " "))
    }
    catch {
        Write-Warning ("cargo publish failed: {0}" -f $_)
    }
}

if ($Tag -and $Version) {
    # If you git push a tag, this will trigger the act_runner and the release workflow on your self-hosted runner (see .gitea/workflows/release.yml)
    $actRunnerScript = Join-Path  $PSScriptRoot 'act_runner.ps1'
    if (Test-Path $actRunnerScript) {
        Write-Host "Ensuring act_runner is running..."
        & $actRunnerScript
    }
    else {
        Write-Warning "act_runner.ps1 not found at $actRunnerScript. Skipping runner start."
    }
    git tag -f "v$Version"
    git push -f origin "v$Version"

    Write-Host "Pushed tag: $Version."
}
