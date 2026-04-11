#!/usr/bin/env pwsh
# Starts the Windows-host Gitea act_runner if not already running.
# Usage: ./scripts/act_runner.ps1 -Config "C:/Users/Admin/Documents/gitea/runner.yaml" -RunnerPath "C:/Users/Admin/Documents/gitea/act_runner.exe" -ServerUrl "https://git.ldlda.com/" -Token "<reg token>" -Labels "self-hosted,windows:host"
param(
    [string]$RunnerPath = "$HOME/Documents/gitea/act_runner.exe",
    # Config is a FILE path (e.g., runner.yaml). Parent folder will be created if missing.
    [string]$Config = (Join-Path (Split-Path -Parent $PSScriptRoot) 'ar_data/config.yaml'),
    [string]$ServerUrl,
    [string]$Token,
    [string]$Labels = "self-hosted,windows:host",
    # Opt-in to non-interactive configure; by default we print the command for you to run manually.
    [switch]$ForceConfigure,
    # When -Attach, run in the foreground to see logs (good for first-time troubleshooting)
    [switch]$Attach,
    # Run one job then exit (great for CI tag pushes)
    [switch]$Once
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $RunnerPath)) {
    Write-Error "Runner not found at $RunnerPath"
}

$running = Get-Process | Where-Object { $_.Path -eq $RunnerPath } | Select-Object -First 1
if ($running) {
    Write-Host "act_runner already running (PID=$($running.Id))"
    exit 0
}

# Ensure config exists
$configFile = $Config
$configDir = Split-Path -Parent $configFile
if (-not (Test-Path $configDir)) { New-Item -ItemType Directory -Force -Path $configDir | Out-Null }

if (-not (Test-Path $configFile)) {
    if (-not $ServerUrl -or -not $Token) {
        Write-Host "Config not found at $configFile. Provide -ServerUrl and -Token for non-interactive setup, or run register manually:"
        Write-Host "`t$RunnerPath register --config $configFile"
        exit 2
    }
    if ($ForceConfigure) {
        Write-Host "Config not found. Initializing non-interactively (ForceConfigure)..."
        $configParent = Split-Path -Parent $configFile
        Push-Location $configParent
        & $RunnerPath register --no-interactive --config $configFile --instance $ServerUrl --token $Token --labels $Labels
        Pop-Location
    }
    else {
        Write-Host "Config not found. Run this manually once (Windows-host runner labels):"
        Write-Host "`t$RunnerPath register --no-interactive --config `"$configFile`" --instance `"$ServerUrl`" --token `"$Token`" --labels `"$Labels`""
        exit 2
    }
}

if ($Attach -or $Once) {
    Write-Host "Starting act_runner $(if ($Once) {'(once mode)'} else {'(attached)'})... $(if (-not $Once) {'Ctrl+C to stop'})"
    $configParent = Split-Path -Parent $configFile
    Push-Location $configParent
    $daemonArgs = @('daemon', '--config', $configFile)
    if ($Once) { $daemonArgs += '--once' }
    & $RunnerPath @daemonArgs
    Pop-Location
}
else {
    $configParent = Split-Path -Parent $configFile
    Push-Location $configParent
    $proc = Start-Process -FilePath $RunnerPath -ArgumentList @("daemon", "--config", $configFile) -WindowStyle Hidden -PassThru
    Pop-Location
    Write-Host "act_runner started (PID=$($proc.Id))."
}
