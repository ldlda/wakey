#!/usr/bin/env pwsh
# Run with pwsh -NoProfile -File scripts/tests/dev_push.ps1. No router is contacted.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$fixture = Join-Path ([IO.Path]::GetTempPath()) "wakey-dev-push-$([guid]::NewGuid())"

function Assert($condition, $message) {
    if (-not $condition) { throw $message }
}

try {
    New-Item -ItemType Directory -Path "$fixture/scripts" -Force | Out-Null
    Copy-Item "$repo/scripts/dev_push.ps1" "$fixture/scripts/dev_push.ps1"
    Copy-Item "$repo/scripts/lib.ps1" "$fixture/scripts/actual-lib.ps1"
    foreach ($file in @('remote_deploy_wakey.sh', 'init/openwrt/wakey', 'hotplug/openwrt/dhcp/95-wakey', 'hotplug/openwrt/neigh/95-wakey')) {
        $dest = "$fixture/scripts/$file"
        New-Item -ItemType Directory -Path (Split-Path -Parent $dest) -Force | Out-Null
        Copy-Item "$repo/scripts/$file" $dest
    }
    # Override only external operations; keep path resolution and quoting real.
    @'
. "$PSScriptRoot/actual-lib.ps1"
function cargo {
    $global:DeployTest.Build = @($args)
    $global:LASTEXITCODE = $global:DeployTest.BuildExit
}
function Invoke-Scp {
    param($Local, $Dest, $Pass, $HostKey, $Port, [switch]$Quiet, [switch]$ForcePassword)
    $global:DeployTest.Uploads.Add([pscustomobject]@{ Local = $Local; Dest = $Dest })
    if ($global:DeployTest.Execute) {
        Copy-Item -LiteralPath $Local -Destination ($Dest -split ':', 2)[1]
    }
}
function Invoke-Ssh {
    param($Cmd, $User, $Remote, $Pass, $Port, [switch]$Quiet, [switch]$ForcePassword, $Stdin)
    $global:DeployTest.Command = $Cmd
    $global:DeployTest.Body = $Stdin
    if (-not $Cmd.StartsWith('sh -s -- ')) { throw 'Deployment must use SSH stdin with sh -s' }
    # Parse exactly what would reach the remote shell, without executing it.
    $Stdin | & sh -n
    if ($LASTEXITCODE -ne 0) { throw 'Generated deployment shell is invalid' }
    if ($global:DeployTest.Execute) {
        $Stdin | & sh -c $Cmd
        if ($LASTEXITCODE -ne 0) { throw 'Local fixture deployment failed' }
    }
}
'@ | Set-Content "$fixture/scripts/lib.ps1"

    function Run-Deploy($options = @{}, $binaries = @('wakey', 'wakey-agent'), $buildExit = 0, $execute = $false) {
        $global:DeployTest = @{
            Build = @(); BuildExit = $buildExit; Execute = $execute
            Uploads = [System.Collections.Generic.List[object]]::new()
            Command = $null
        }
        $output = "$fixture/target/test/release"
        if (Test-Path $output) { Remove-Item -Recurse -Force $output }
        New-Item -ItemType Directory -Path $output -Force | Out-Null
        foreach ($binary in $binaries) { New-Item -ItemType File -Path "$output/$binary" | Out-Null }
        & "$fixture/scripts/dev_push.ps1" -Pass test-only -HostName router -Target test -Quiet:$false @options
    }

    Run-Deploy
    Assert ($global:DeployTest.Uploads.Count -eq 5) 'Default deploy must upload both binaries and three support files'
    Assert (($global:DeployTest.Build -join ' ') -eq 'build --release --target test -p wakey -p wakey-agent') 'Default build changed'
    Assert ($global:DeployTest.Command.Contains("'/root/.bin/wakey.tmp' '/root/.bin/wakey' '0'")) 'CLI must not restart before agent replacement'
    Assert ($global:DeployTest.Command.Contains("'/root/.bin/wakey-agent.tmp' '/root/.bin/wakey-agent' '1'")) 'Agent must restart by default'
    Assert ($global:DeployTest.Body -ceq (Get-Content -Raw "$repo/scripts/remote_deploy_wakey.sh")) 'Deployment must stream the checked-in file unchanged'

    Run-Deploy @{ BinariesOnly = $true }
    Assert ($global:DeployTest.Uploads.Count -eq 2) 'Binary-only deploy uploaded support files'
    Assert (-not $global:DeployTest.Command.Contains('--install')) 'Binary-only deploy contains support installation'

    foreach ($package in @('wakey', 'wakey-agent')) {
        Run-Deploy @{ BinariesOnly = $true; Package = @($package, $package); Restart = $false } @($package)
        Assert ($global:DeployTest.Uploads.Count -eq 1) 'Single-package deploy uploaded another file'
        Assert (($global:DeployTest.Build -join ' ') -eq "build --release --target test -p $package") 'Package selection or deduplication failed'
        Assert ($global:DeployTest.Command.Contains("'/root/.bin/$package.tmp' '/root/.bin/$package' '0'")) 'Restart override lost'
    }

    Run-Deploy @{ SkipInitScript = $true }
    Assert ($global:DeployTest.Uploads.Count -eq 4) 'SkipInitScript must still upload hooks'

    Run-Deploy @{ BinariesOnly = $true; Package = @('wakey-agent'); AgentRemotePath = "/tmp/owner's bin/agent" } @('wakey-agent')
    Assert ($global:DeployTest.Command.Contains('owner')) 'Custom path lost'

    # Execute only binary replacement inside the temporary fixture, with restart
    # disabled. This also checks that the helper's exit doesn't skip binary two.
    $cliPath = "$fixture/installed cli"
    $agentPath = "$fixture/owner's agent"
    Run-Deploy @{ BinariesOnly = $true; Restart = $false; RemotePath = $cliPath; AgentRemotePath = $agentPath } @('wakey', 'wakey-agent') 0 $true
    Assert ((Test-Path -LiteralPath $cliPath) -and (Test-Path -LiteralPath $agentPath)) 'Streamed helper did not install both binaries'
    Assert (-not (Test-Path -LiteralPath "$agentPath.tmp")) 'Temporary upload was not moved'

    $supportTemp = "$fixture/support.tmp"
    $supportDest = "$fixture/hook dir/owner's hook"
    [IO.File]::WriteAllText($supportTemp, "#!/bin/sh`r`necho hook`r`n")
    & sh "$fixture/scripts/remote_deploy_wakey.sh" --batch --install $supportTemp $supportDest
    Assert ($LASTEXITCODE -eq 0) 'Support installation failed'
    Assert (-not (Get-Content -Raw -LiteralPath $supportDest).Contains("`r")) 'Support CRLF was not normalized'
    & sh -c 'test -x "$1"' sh $supportDest
    Assert ($LASTEXITCODE -eq 0) 'Installed support file is not executable'

    New-Item -ItemType File -Path "$fixture/legacy.tmp" | Out-Null
    & sh "$fixture/scripts/remote_deploy_wakey.sh" "$fixture/legacy.tmp" "$fixture/legacy" 0
    Assert ($LASTEXITCODE -eq 0 -and (Test-Path "$fixture/legacy")) 'Original helper invocation broke'

    $failed = $false
    try { Run-Deploy @{} @() 1 } catch { $failed = $_.Exception.Message -like '*build failed*' }
    Assert $failed 'Build failure did not abort deployment'
    Assert ($global:DeployTest.Uploads.Count -eq 0 -and $null -eq $global:DeployTest.Command) 'Failed build attempted deployment'
    Write-Host 'dev_push tests passed'
} finally {
    Remove-Variable DeployTest -Scope Global -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $fixture
}
