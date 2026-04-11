#!/usr/bin/env pwsh
[System.Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidDefaultValueSwitchParameter', "", 
    Justification = 'Default true is intentional for fast dev loop')]
param(
    [ValidateSet("cargo", "cross")]
    [string]$Cargo = "cargo",
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
    [switch]$Restart = $true,
    [switch]$Quiet = $true
)

$ErrorActionPreference = 'Stop'
try { $PSStyle.OutputRendering = 'Host' } catch {}

. "$PSScriptRoot/lib.ps1"

$Pass = Get-DefaultPassword $Pass

# Backward compatibility: allow HostName in the form host:port.
$hostPort = Split-HostPort -HostName $HostName -DefaultPort $Port
$HostName = $hostPort.Host
$Port = $hostPort.Port

function Get-DeployScript {
    param($DeployPreferred, $DeployTmp, $RemoteTmp, $RemotePath, $RestartFlag)
    return @"
DEPLOY=$DeployPreferred;
if [ ! -x "`$DEPLOY" ] && [ -f $DeployTmp ]; then
  sed -i "s/\r$//" $DeployTmp 2>/dev/null || true
  chmod +x $DeployTmp
  DEPLOY=$DeployTmp
fi
sh "`$DEPLOY" $RemoteTmp $RemotePath $RestartFlag
"@
}

#---- Main Flow ----
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot

try {
    Write-Host "[build] $Cargo build --release --target $Target -p wakey -p wakey-agent" -ForegroundColor Cyan
    . "$Cargo" build --release --target $Target -p wakey -p wakey-agent
    if ($LASTEXITCODE -ne 0) { throw "$Cargo build failed ($LASTEXITCODE)" }

    $localBin = Join-Path $repoRoot "target/$Target/release/$BinName"
    $localAgentBin = Join-Path $repoRoot "target/$Target/release/$AgentBinName"
    if (-not (Test-Path $localBin)) { throw "binary not found: $localBin" }
    if (-not (Test-Path $localAgentBin)) { throw "binary not found: $localAgentBin" }

    $remoteTmp = "$RemotePath.tmp"
    $agentRemoteTmp = "$AgentRemotePath.tmp"
    $destTmp = "$User@${HostName}:$remoteTmp"
    $agentDestTmp = "$User@${HostName}:$agentRemoteTmp"
    $initTmp = '/var/tmp/wakey.init.tmp'
    $initDestTmp = "$User@${HostName}:$initTmp"
    $localDeploy = Join-Path $repoRoot 'scripts/remote_deploy_wakey.sh'
    $localInit = Join-Path $repoRoot 'scripts/init/openwrt/wakey'
    $deployTmp = '/var/tmp/remote_deploy_wakey.sh'
    $deployPreferred = '/root/.bin/remote_deploy_wakey.sh'

    # Push binaries
    Invoke-Scp -Local $localBin -Dest $destTmp -Pass $Pass -HostKey $HostKey -Port $Port -Quiet:$Quiet -ForcePassword:$ForcePassword
    Invoke-Scp -Local $localAgentBin -Dest $agentDestTmp -Pass $Pass -HostKey $HostKey -Port $Port -Quiet:$Quiet -ForcePassword:$ForcePassword

    # Push OpenWrt init script unless explicitly skipped
    if (-not $SkipInitScript -and (Test-Path $localInit)) {
        Invoke-Scp -Local $localInit -Dest $initDestTmp -Pass $Pass -HostKey $HostKey -Port $Port -Quiet:$Quiet -ForcePassword:$ForcePassword
    }

    # Push deploy helper if exists
    if (Test-Path $localDeploy) {
        Invoke-Scp -Local $localDeploy -Dest "$User@${HostName}:$deployTmp" -Pass $Pass -HostKey $HostKey -Port $Port -Quiet:$Quiet -ForcePassword:$ForcePassword
    }

    # Build and run remote deploy command
    $restartFlag = $(if ($Restart) { '1' } else { '0' })
    $script = @"
$(Get-DeployScript $deployPreferred $deployTmp $remoteTmp $RemotePath 0)
$(Get-DeployScript $deployPreferred $deployTmp $agentRemoteTmp $AgentRemotePath $restartFlag)
$(if (-not $SkipInitScript) {
@"
if [ -f $initTmp ]; then
    sed -i "s/\r$//" $initTmp 2>/dev/null || true
    if command -v install >/dev/null 2>&1; then
        install -m 0755 $initTmp $RemoteInitPath
    else
        cp -f $initTmp $RemoteInitPath
        chmod 0755 $RemoteInitPath
    fi
fi
"@
})
"@
    $remoteCmd = "sh -lc '$($script -replace "`r",'')'"
    if ($Quiet) { $remoteCmd += " >/dev/null 2>&1" }

    Invoke-Ssh -Cmd $remoteCmd -User $User -Remote $HostName -Pass $Pass -Port $Port -Quiet:$Quiet -ForcePassword:$ForcePassword

    Write-Host "done ✔" -ForegroundColor Green
}
finally {
    Pop-Location
}
