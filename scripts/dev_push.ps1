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

function Invoke-Ext {
    param($Exe, $Arguments, $Label)
    $displayArgs = $Arguments.Clone()
    for ($i = 0; $i -lt $displayArgs.Count; $i++) {
        if ($displayArgs[$i] -eq '-pw' -and ($i + 1) -lt $displayArgs.Count) {
            $displayArgs[$i + 1] = '****'
        }
    }
    Write-Host ("[{0}] {1} {2}" -f $Label, $Exe, ($displayArgs -join ' ')) -ForegroundColor Cyan

    $out = & $Exe @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Error ("{0} failed ({1}):`n{2}" -f $Label, $LASTEXITCODE, ($out -join "`n"))
        throw ("{0} failed ({1})" -f $Label, $LASTEXITCODE)
    }
    return $out
}

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
"@ # -replace "`r", "" -replace "`r`n", "`n"

}

function Invoke-Scp {
    param($Local, $Dest, $Pass, $HostKey, [switch]$Quiet, [switch]$Recurse)
    if ($pscp = Get-Command pscp.exe -ErrorAction SilentlyContinue) {
        $arguments = @('-scp')
        if ($Quiet) { $arguments += '-q' }
        if ($Recurse) { $arguments += '-r' }
        if ($HostKey) { $arguments += @('-batch', '-hostkey', $HostKey) }
        if ($Pass) { $arguments += @('-pw', $Pass) }
        $arguments += @($Local, $Dest)
        Invoke-Ext -Exe $pscp.Path -Arguments $arguments -Label 'scp'
    }
    else {
        $arguments = @('-O')
        if ($Quiet) { $arguments += '-q' }
        if ($Recurse) { $arguments += '-r' }
        $arguments += @($Local, $Dest)
        Invoke-Ext -Exe 'scp' -Arguments $arguments -Label 'scp'
    }
}

function Invoke-Ssh {
    param($Cmd, $User, $Remote, $Pass, [switch]$Quiet)
    $Cmd = $Cmd -replace "`r`n", "`n" -replace "`r", ""
    if ($plink = Get-Command plink.exe -ErrorAction SilentlyContinue) {
        $arguments = @('-batch', '-ssh')
        if ($Pass) { $arguments += @('-pw', $Pass) }
        $arguments += "$User@$Remote", $Cmd
        Invoke-Ext -Exe $plink.Path -Arguments $arguments -Label 'ssh'
    }
    else {
        $arguments = @()
        if ($Quiet) { $arguments += '-q' }
        $arguments += "$User@$Remote", $Cmd
        Invoke-Ext -Exe 'ssh' -Arguments $arguments -Label 'ssh'
    }
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
        $remoteDir = Split-Path $RemotePath -Parent
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
