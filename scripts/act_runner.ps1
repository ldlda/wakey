# Starts the Gitea act_runner if not already running.
# Usage: ./scripts/act_runner.ps1 -Config "C:/Users/Admin/Documents/gitea/.runner" -RunnerPath "C:/Users/Admin/Documents/gitea/act_runner.exe"
param(
    [string]$RunnerPath = "$HOME/Documents/gitea/act_runner.exe",
    [string]$Config = "$HOME/Documents/gitea/.runner"
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
if (-not (Test-Path $Config)) {
    Write-Host "Config not found. Initializing..."
    & $RunnerPath configure --no-interactive --config $Config
}

Start-Process -FilePath $RunnerPath -ArgumentList @("daemon", "--config", $Config) -WindowStyle Hidden
Write-Host "act_runner started."
