[System.Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidDefaultValueSwitchParameter', "", 
    Justification = 'Default true is intentional for fast dev loop')]
param(
    [string]$Pass,
    [string]$HostName = "192.168.100.1",
    [string]$User = "root",
    [string]$RemotePath = "/root/.bin/wakey",
    [string]$Target = "armv7-unknown-linux-musleabihf",
    [string]$BinName = "wakey",
    [string]$HostKey,
    [switch]$Restart = $true,
    [switch]$Quiet = $true
)

$ErrorActionPreference = 'Stop'
try { $PSStyle.OutputRendering = 'Host' } catch {}

. "$PSScriptRoot/lib.ps1"

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
    Write-Host "[build] cargo build --release --target $Target" -ForegroundColor Cyan
    cargo build --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }

    $localBin = Join-Path $repoRoot "target/$Target/release/$BinName"
    if (-not (Test-Path $localBin)) { throw "binary not found: $localBin" }

    $remoteTmp = "$RemotePath.tmp"
    $destTmp = "$User@${HostName}:$remoteTmp"
    $localDeploy = Join-Path $repoRoot 'scripts/remote_deploy_wakey.sh'
    $deployTmp = '/var/tmp/remote_deploy_wakey.sh'
    $deployPreferred = '/root/.bin/remote_deploy_wakey.sh'

    # Push main binary
    Invoke-Scp -Local $localBin -Dest $destTmp -Pass $Pass -HostKey $HostKey -Quiet:$Quiet

    # Push static assets
    $localStatic = Join-Path $repoRoot "static"
    if (Test-Path $localStatic) {
        # Assuming RemotePath is like /root/.bin/wakey, we want /root/.bin/static
        # So we push 'static' directory to /root/.bin/
        $remoteDir = (Split-Path $RemotePath -Parent) -replace '\\', '/'
        # Ensure remote dir exists (ssh mkdir -p)
        Invoke-Ssh -Cmd "mkdir -p $remoteDir" -User $User -Remote $HostName -Pass $Pass -Quiet:$Quiet
        
        # SCP -r static user@host:/root/.bin/
        # Note: pscp/scp behavior: if dest is a dir, it copies the source dir INTO it.
        Invoke-Scp -Local $localStatic -Dest "$User@${HostName}:$remoteDir/" -Pass $Pass -HostKey $HostKey -Quiet:$Quiet -Recurse
    }

    # Push deploy helper if exists
    if (Test-Path $localDeploy) {
        Invoke-Scp -Local $localDeploy -Dest "$User@${HostName}:$deployTmp" -Pass $Pass -HostKey $HostKey -Quiet:$Quiet
    }

    # Build and run remote deploy command
    $restartFlag = $(if ($Restart) { '1' } else { '0' })
    $script = Get-DeployScript $deployPreferred $deployTmp $remoteTmp $RemotePath $restartFlag
    $remoteCmd = "sh -lc '$($script -replace "`r",'')'"
    if ($Quiet) { $remoteCmd += " >/dev/null 2>&1" }

    Invoke-Ssh -Cmd $remoteCmd -User $User -Remote $HostName -Pass $Pass -Quiet:$Quiet

    Write-Host "done ✔" -ForegroundColor Green
}
finally {
    Pop-Location
}
