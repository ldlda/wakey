#!/usr/bin/env pwsh
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
    [string]$Registry,
    [ValidateSet('wsl', 'windows', 'none')]
    [string]$RunnerMode = 'wsl',
    [string]$WslDistro = $(if ($env:WAKEY_WSL_DISTRO) { $env:WAKEY_WSL_DISTRO } else { 'FedoraLinux-43' })
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/lib.ps1"

# If no version provided, deduce from Cargo.toml
$versionProvided = [bool]$Version
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

# Only write version if it was explicitly provided (not deduced)
if ($versionProvided) {
    $pattern = '(?m)^version\s*=\s*"[^"]+"'
    $replacement = ('version = "{0}"' -f $Version)
    (Get-Content Cargo.toml -Raw) -replace $pattern, $replacement | Set-Content Cargo.toml -Encoding UTF8 -NoNewline
}

# Ensure build ok
$env:SQLX_OFFLINE = 'true'
cargo fmt
cargo build --release

# Publish crate (only if -Publish and registry auth is available)
# Publish crate (only if -Publish). Relies on your Cargo config/credentials.
if ($Publish) {
    if (-not $Registry) {
        $Registry = 'gitea'
        Write-Host 'No -Registry provided; defaulting publish target to registry: gitea'
    }

    $publishOrder = @(
        'lda-ipjs',
        'wakey-core',
        'wakey-linux',
        'wakey',
        'wakey-agent',
        'wakey-control-plane'
    )

    foreach ($pkg in $publishOrder) {
        $pubArgs = @('publish', '-p', $pkg)
        if ($Registry) { $pubArgs += @('--registry', $Registry) }

        Write-Host ("publishing {0}..." -f $pkg)
        $publishOutput = & cargo @pubArgs 2>&1
        $publishOutput | ForEach-Object { Write-Host $_ }

        if ($LASTEXITCODE -ne 0) {
            $joined = ($publishOutput | Out-String)
            if ($joined -match 'already exists' -or $joined -match 'already uploaded') {
                Write-Warning ("skip {0}: version already present in registry" -f $pkg)
                continue
            }

            throw ("cargo publish failed for {0}. ran: cargo {1}" -f $pkg, ($pubArgs -join ' '))
        }

        Write-Host ("published {0}." -f $pkg)
    }
}

if ($Tag -and $Version) {
    git tag -f -a "v$Version" -m "Release $Version"
    git push -f origin "v$Version"
    Write-Host "Pushed tag: v$Version"

    switch ($RunnerMode) {
        'wsl' {
            if ($IsWindows) {
                $runnerScript = Join-Path $PSScriptRoot 'act_runner_wsl.ps1'
                if (Test-Path $runnerScript) {
                    Write-Host "Starting Fedora/WSL act_runner via Windows wrapper (once mode)..."
                    & $runnerScript -Distro $WslDistro -Action start -Once
                }
                else {
                    Write-Warning "act_runner_wsl.ps1 not found at $runnerScript. Skipping runner."
                }
            }
            else {
                $runnerScript = Join-Path $PSScriptRoot 'act_runner_fedora.sh'
                if (Test-Path $runnerScript) {
                    Write-Host "Starting Fedora runner directly (once mode)..."
                    & bash $runnerScript start --once
                }
                else {
                    Write-Warning "act_runner_fedora.sh not found at $runnerScript. Skipping runner."
                }
            }
        }
        'windows' {
            $runnerScript = Join-Path $PSScriptRoot 'act_runner.ps1'
            if (Test-Path $runnerScript) {
                Write-Host "Starting Windows act_runner (once mode) to process release job..."
                & $runnerScript -Once
            }
            else {
                Write-Warning "act_runner.ps1 not found at $runnerScript. Skipping runner."
            }
        }
        'none' {
            Write-Host "RunnerMode=none; not starting a local runner helper."
        }
    }
}
