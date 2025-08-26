[System.Diagnostics.CodeAnalysis.SuppressMessage('PSAvoidDefaultValueSwitchParameter', 'Default true is intentional for fast dev loop')]
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
    param(
        [Parameter(Mandatory = $true)][string]$Exe,
        [Parameter(Mandatory = $true)][string[]]$Args,
        [Parameter(Mandatory = $true)][string]$Label
    )
    $displayArgs = @($Args)
    for ($i = 0; $i -lt $displayArgs.Count; $i++) {
        if ($displayArgs[$i] -eq '-pw' -and ($i + 1) -lt $displayArgs.Count) { $displayArgs[$i + 1] = '****' } # lowkey leaked my password
    }
    Write-Host ("[{0}] {1} {2}" -f $Label, $Exe, ($displayArgs -join ' ')) -ForegroundColor Cyan
    $out = & $Exe @Args 2>&1
    $code = $LASTEXITCODE
    if ($code -ne 0) {
        Write-Error ("{0} failed ({1}):`n{2}" -f $Label, $code, ($out -join "`n"))
        throw ("{0} failed ({1})" -f $Label, $code)
    }
    return $out
}

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    Write-Host "[build] cargo build --release --target $Target" -ForegroundColor Cyan
    cargo build --release --target $Target
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }

    $local = Join-Path $repoRoot ("target/" + $Target + "/release/" + $BinName)
    if (-not (Test-Path $local)) { throw "binary not found: $local" }

    $remoteTmp = "$RemotePath.tmp"
    $destTmp = "{0}@{1}:{2}" -f $User, $HostName, $remoteTmp
    $localDeploy = Join-Path $repoRoot 'scripts/remote_deploy_wakey.sh'
    $deployTmp = '/var/tmp/remote_deploy_wakey.sh'
    $destDeployTmp = "{0}@{1}:{2}" -f $User, $HostName, $deployTmp
    $deployPreferred = '/root/.bin/remote_deploy_wakey.sh'

    if ($Pass) {
        $pscp = Get-Command pscp.exe -ErrorAction SilentlyContinue
        if ($pscp) {
            $pscpArgs = @()
            if ($Quiet) { $pscpArgs += '-q' }
            if ($HostKey) { $pscpArgs += @('-batch', '-scp', '-hostkey', $HostKey, '-pw', $Pass, $local, $destTmp) }
            else { $pscpArgs += @('-scp', '-pw', $Pass, $local, $destTmp) }
            Invoke-Ext -Exe $pscp.Path -Args $pscpArgs -Label 'push'

            $plink = Get-Command plink.exe -ErrorAction SilentlyContinue
            if (Test-Path $localDeploy) {
                $pscpArgsD = @()
                if ($Quiet) { $pscpArgsD += '-q' }
                if ($HostKey) { $pscpArgsD += @('-batch', '-scp', '-hostkey', $HostKey, '-pw', $Pass, $localDeploy, $destDeployTmp) }
                else { $pscpArgsD += @('-scp', '-pw', $Pass, $localDeploy, $destDeployTmp) }
                Invoke-Ext -Exe $pscp.Path -Args $pscpArgsD -Label 'push-deploy'
            }
            $remoteCmdCore = @'
DEPLOY=$DEPLOY_PREFERRED
if [ ! -x "$DEPLOY" ] && [ -f $DEPLOY_TMP ]; then
 sed -i "s/\r$//" $DEPLOY_TMP 2>/dev/null || true
 chmod +x $DEPLOY_TMP; DEPLOY=$DEPLOY_TMP
fi
sh "$DEPLOY" $REMOTE_TMP $REMOTE_PATH $DO_RESTART
'@
            $remoteCmdCore = $remoteCmdCore.Replace('$DEPLOY_PREFERRED', $deployPreferred).Replace('$DEPLOY_TMP', $deployTmp).Replace('$REMOTE_TMP', $remoteTmp).Replace('$REMOTE_PATH', $RemotePath)
            $remoteCmdCore = if ($Restart) { $remoteCmdCore.Replace('$DO_RESTART', '1') } else { $remoteCmdCore.Replace('$DO_RESTART', '0') }
            $remoteCmdCore = ($remoteCmdCore -replace "`r", "")
            $remoteCmd = "sh -lc '$remoteCmdCore'"
            if ($Quiet) { $remoteCmd = "$remoteCmd >/dev/null 2>&1" }
            if ($plink) {
                $plinkArgs = @('-batch', '-ssh', '-pw', $Pass, "$User@$HostName", $remoteCmd)
                Invoke-Ext -Exe $plink.Path -Args $plinkArgs -Label 'ssh'
            }
            else {
                $sshArgs = @()
                if ($Quiet) { $sshArgs += '-q' }
                $sshArgs += @("$User@$HostName", $remoteCmd)
                Invoke-Ext -Exe 'ssh' -Args $sshArgs -Label 'ssh'
            }
        }
        else {
            Write-Warning "pscp.exe not found. Falling back to scp (you may be prompted for a password)."
            $scpArgs = @('-O')
            if ($Quiet) { $scpArgs += '-q' }
            $scpArgs += @($local, $destTmp)
            Invoke-Ext -Exe 'scp' -Args $scpArgs -Label 'push'
            if (Test-Path $localDeploy) {
                $scpArgsD = @('-O')
                if ($Quiet) { $scpArgsD += '-q' }
                $scpArgsD += @($localDeploy, $destDeployTmp)
                Invoke-Ext -Exe 'scp' -Args $scpArgsD -Label 'push-deploy'
            }
            $remoteCmdCore = @'
DEPLOY=$DEPLOY_PREFERRED
if [ ! -x "$DEPLOY" ] && [ -f $DEPLOY_TMP ]; then
 sed -i "s/\r$//" $DEPLOY_TMP 2>/dev/null || true
 chmod +x $DEPLOY_TMP; DEPLOY=$DEPLOY_TMP
fi
sh "$DEPLOY" $REMOTE_TMP $REMOTE_PATH $DO_RESTART
'@
            $remoteCmdCore = $remoteCmdCore.Replace('$DEPLOY_PREFERRED', $deployPreferred).Replace('$DEPLOY_TMP', $deployTmp).Replace('$REMOTE_TMP', $remoteTmp).Replace('$REMOTE_PATH', $RemotePath)
            $remoteCmdCore = if ($Restart) { $remoteCmdCore.Replace('$DO_RESTART', '1') } else { $remoteCmdCore.Replace('$DO_RESTART', '0') }
            $remoteCmdCore = ($remoteCmdCore -replace "`r", "")
            $remoteCmd = "sh -lc '$remoteCmdCore'"
            if ($Quiet) { $remoteCmd = "$remoteCmd >/dev/null 2>&1" }
            $sshArgs = @()
            if ($Quiet) { $sshArgs += '-q' }
            $sshArgs += @("$User@$HostName", $remoteCmd)
            Invoke-Ext -Exe 'ssh' -Args $sshArgs -Label 'ssh'
        }
    }
    else {
        $scpArgs = @('-O')
        if ($Quiet) { $scpArgs += '-q' }
        $scpArgs += @($local, $destTmp)
        Invoke-Ext -Exe 'scp' -Args $scpArgs -Label 'push'
        if (Test-Path $localDeploy) {
            $scpArgsD = @('-O')
            if ($Quiet) { $scpArgsD += '-q' }
            $scpArgsD += @($localDeploy, $destDeployTmp)
            Invoke-Ext -Exe 'scp' -Args $scpArgsD -Label 'push-deploy'
        }
        $remoteCmdCore = @'
DEPLOY=$DEPLOY_PREFERRED
if [ ! -x "$DEPLOY" ] && [ -f $DEPLOY_TMP ]; then sed -i "s/\r$//" $DEPLOY_TMP 2>/dev/null || true; chmod +x $DEPLOY_TMP; DEPLOY=$DEPLOY_TMP; fi
sh "$DEPLOY" $REMOTE_TMP $REMOTE_PATH $DO_RESTART
'@
        $remoteCmdCore = $remoteCmdCore.Replace('$DEPLOY_PREFERRED', $deployPreferred).Replace('$DEPLOY_TMP', $deployTmp).Replace('$REMOTE_TMP', $remoteTmp).Replace('$REMOTE_PATH', $RemotePath)
        $remoteCmdCore = if ($Restart) { $remoteCmdCore.Replace('$DO_RESTART', '1') } else { $remoteCmdCore.Replace('$DO_RESTART', '0') }
        $remoteCmdCore = ($remoteCmdCore -replace "`r", "")
        $remoteCmd = "sh -lc '$remoteCmdCore'"
        if ($Quiet) { $remoteCmd = "$remoteCmd >/dev/null 2>&1" }
        $sshArgs = @()
        if ($Quiet) { $sshArgs += '-q' }
        $sshArgs += @("$User@$HostName", $remoteCmd)
        Invoke-Ext -Exe 'ssh' -Args $sshArgs -Label 'ssh'
    }

    Write-Host "done ✔" -ForegroundColor Green
}
finally {
    Pop-Location
}
