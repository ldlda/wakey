#!/usr/bin/env pwsh
[System.Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidDefaultValueSwitchParameter', "", 
    Justification = 'Default true is intentional for fast dev loop')]
param(
    [ValidateSet("cargo", "cross")]
    [string]$Cargo = "cargo",
    [ValidateSet("wakey", "wakey-agent")]
    [ValidateNotNullOrEmpty()]
    [string[]]$Package = @("wakey", "wakey-agent"),
    [string]$Pass,
    [string]$HostName = "192.168.100.1",
    [int]$Port = 22,
    [string]$User = "root",
    [string]$RemotePath = "/root/.bin/wakey",
    [string]$AgentRemotePath = "/root/.bin/wakey-agent",
    [string]$RemoteInitPath = "/etc/init.d/wakey",
    [string]$Target = "armv7-unknown-linux-musleabihf",
    [string]$BinName = "wakey",
    [string]$AgentBinName = "wakey-agent",
    [string]$HostKey,
    [switch]$ForcePassword,
    [switch]$SkipInitScript,
    [switch]$BinariesOnly,
    [switch]$Restart = $true,
    [switch]$Quiet = $true
)

$ErrorActionPreference = 'Stop'
try { $PSStyle.OutputRendering = 'Host' } catch {}

. "$PSScriptRoot/lib.ps1"

function Build-Packages {
    param([string[]]$Packages)
    $buildArgs = @('build', '--release', '--target', $Target)
    foreach ($pkg in $Packages) { $buildArgs += @('-p', $pkg) }
    Write-Host "[build] $Cargo $($buildArgs -join ' ')" -ForegroundColor Cyan
    & $Cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "$Cargo build failed ($LASTEXITCODE)" }

}

function New-DeploymentPlan {
    param([string[]]$Packages)
    # Keep CLI first so replacing both binaries restarts the service only once.
    $binaries = @(
        @{ Package = 'wakey'; Name = $BinName; Destination = $RemotePath }
        @{ Package = 'wakey-agent'; Name = $AgentBinName; Destination = $AgentRemotePath }
    ) | Where-Object { $_.Package -in $Packages } | ForEach-Object {
        [pscustomobject]@{
            Local = Join-Path $repoRoot "target/$Target/release/$($_.Name)"
            Temp = "$($_.Destination).tmp"
            Destination = $_.Destination
        }
    }
    $support = @(
        if (-not $BinariesOnly) {
            if (-not $SkipInitScript) {
                @{ Source = 'init/openwrt/wakey'; Temp = '/var/tmp/wakey.init.tmp'; Destination = $RemoteInitPath }
            }
            @{ Source = 'hotplug/openwrt/dhcp/95-wakey'; Temp = '/var/tmp/wakey.hotplug.dhcp.tmp'; Destination = '/etc/hotplug.d/dhcp/95-wakey' }
            @{ Source = 'hotplug/openwrt/neigh/95-wakey'; Temp = '/var/tmp/wakey.hotplug.neigh.tmp'; Destination = '/etc/hotplug.d/neigh/95-wakey' }
        }
    ) | ForEach-Object {
        $local = Join-Path $PSScriptRoot $_.Source
        if (Test-Path -LiteralPath $local) {
            [pscustomobject]@{ Local = $local; Temp = $_.Temp; Destination = $_.Destination }
        }
    }
    foreach ($binary in $binaries) {
        if (-not (Test-Path -LiteralPath $binary.Local)) { throw "binary not found: $($binary.Local)" }
    }
    [pscustomobject]@{ Binaries = @($binaries); Support = @($support) }
}

function Get-RemoteDeployment {
    param($Plan)
    $deployArgs = @('--batch')
    for ($i = 0; $i -lt $Plan.Binaries.Count; $i++) {
        $binary = $Plan.Binaries[$i]
        $restartFlag = if ($Restart -and $i -eq $Plan.Binaries.Count - 1) { '1' } else { '0' }
        $deployArgs += @('--binary', $binary.Temp, $binary.Destination, $restartFlag)
    }
    foreach ($file in $Plan.Support) {
        $deployArgs += @('--install', $file.Temp, $file.Destination)
    }
    # Only data needs shell quoting; the checked-in shell program travels as stdin.
    $command = 'sh -s -- ' + (($deployArgs | ForEach-Object { Quote-ShArg $_ }) -join ' ')
    if ($Quiet) { $command += ' >/dev/null 2>&1' }
    [pscustomobject]@{
        Script = Get-Content -Raw -LiteralPath "$PSScriptRoot/remote_deploy_wakey.sh"
        Command = $command
    }
}

function Send-DeploymentFiles {
    param($Plan)
    foreach ($file in @($Plan.Binaries) + @($Plan.Support)) {
        Invoke-Scp -Local $file.Local -Dest "$User@${HostName}:$($file.Temp)" -Pass $Pass -HostKey $HostKey -Port $Port -Quiet:$Quiet -ForcePassword:$ForcePassword
    }
}

$Pass = Get-DefaultPassword $Pass
# Backward compatibility: allow HostName in the form host:port.
$hostPort = Split-HostPort -HostName $HostName -DefaultPort $Port
$HostName = $hostPort.Host
$Port = $hostPort.Port
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    Build-Packages ($Package | Select-Object -Unique)
    $plan = New-DeploymentPlan $Package
    # Prepare before uploading so missing local helpers fail without remote changes.
    $deployment = Get-RemoteDeployment $plan
    Send-DeploymentFiles $plan
    Invoke-Ssh -Cmd $deployment.Command -Stdin $deployment.Script -User $User -Remote $HostName -Pass $Pass -Port $Port -Quiet:$Quiet -ForcePassword:$ForcePassword
    Write-Host "done ✔" -ForegroundColor Green
}
finally {
    Pop-Location
}
