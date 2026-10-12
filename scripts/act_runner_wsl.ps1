#!/usr/bin/env pwsh
param(
    [ValidateSet("update", "register", "start", "stop", "status")]
    [string]$Action = "start",
    [string]$Distro = $( $env:WAKEY_WSL_DISTRO ?? "FedoraLinux-43" ),
    [string]$RunnerHome,
    [string]$RunnerBin = "/home/lda/gitea-runner/act_runner",
    [string]$ConfigPath,
    [string]$ServerUrl,
    [string]$Token,
    [string]$Labels = "self-hosted,linux,fedora,wsl,release:host",
    [switch]$Attach,
    [switch]$Once
)

$ErrorActionPreference = "Stop"

. "$PSScriptRoot/lib.ps1"

$scriptWin = Join-Path $PSScriptRoot "act_runner_fedora.sh"
if (-not (Test-Path $scriptWin)) {
    throw "Missing Fedora runner helper: $scriptWin"
}

$scriptLinux = ConvertTo-WslPath -WindowsPath $scriptWin -Distro $Distro
if (-not $scriptLinux) {
    throw "Failed to resolve WSL path for $scriptWin"
}

if (-not $RunnerHome) {
    $repoRootWin = Split-Path -Parent $PSScriptRoot
    $defaultRunnerHomeWin = Join-Path $repoRootWin "ar_data/wsl_runner"
    $RunnerHome = $defaultRunnerHomeWin
}

if (-not $ConfigPath) {
    $repoRootWin = Split-Path -Parent $PSScriptRoot
    $defaultConfigWin = Join-Path $repoRootWin "ar_data/wsl_runner/config.yaml"
    $ConfigPath = $defaultConfigWin
}

$RunnerHome = ConvertTo-WslPath -Path $RunnerHome -Distro $Distro
$RunnerBin = ConvertTo-WslPath -Path $RunnerBin -Distro $Distro
$ConfigPath = ConvertTo-WslPath -Path $ConfigPath -Distro $Distro

$parts = @("bash", (Quote-ShArg $scriptLinux), (Quote-ShArg $Action), "--runner-home", (Quote-ShArg $RunnerHome))
$parts += @("--runner-bin", (Quote-ShArg $RunnerBin), "--config-path", (Quote-ShArg $ConfigPath))
if ($ServerUrl) { $parts += @("--server-url", (Quote-ShArg $ServerUrl)) }
if ($Token) { $parts += @("--token", (Quote-ShArg $Token)) }
if ($Labels) { $parts += @("--labels", (Quote-ShArg $Labels)) }
if ($Attach) { $parts += "--attach" }
if ($Once) { $parts += "--once" }

$cmd = $parts -join " "
Write-Host $cmd
& wsl.exe -d $Distro -e bash -lc $cmd
if ($LASTEXITCODE -ne 0) {
    throw "WSL runner action failed ($LASTEXITCODE)"
}
