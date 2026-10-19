#!/usr/bin/env pwsh
# Run with pwsh -NoProfile -File scripts/tests/ssh_stdin.ps1 on Linux/WSL.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../lib.ps1"

# Exercise the real native-command pipeline without contacting a server.
$body = "printf '%s\n' `"owner's directory`"`nprintf '%s\n' 'second line'"
$output = @(Invoke-Ext -Exe sh -Arguments @('-s') -Label test -Stdin $body)
if (($output -join '|') -cne "owner's directory|second line") { throw 'stdin script changed in transit' }
$output = @(Invoke-Ext -Exe sh -Arguments @('-c', 'printf command-only') -Label test)
if (($output -join '') -cne 'command-only') { throw 'Command-only execution changed' }
$failed = $false
try { Invoke-Ext -Exe sh -Arguments @('-s') -Label test -Stdin 'exit 7' } catch { $failed = $true }
if (-not $failed) { throw 'stdin execution swallowed a failure' }

# Test SSH argument and stdin forwarding independently of native SSH availability.
function Test-IsWsl { return $true }
function Get-SshpassCommand { return $script:Sshpass }
function Invoke-Ext {
    param($Exe, $Arguments, $Label, [string]$Stdin)
    $script:Call = @{ Exe = $Exe; Arguments = $Arguments; Stdin = $Stdin; HasStdin = $PSBoundParameters.ContainsKey('Stdin') }
}
$script:Sshpass = $null
Invoke-Ssh -Cmd 'sh -s' -User root -Remote test -Stdin "printf ok`r`n"
if ($script:Call.Exe -ne 'ssh' -or $script:Call.Stdin.Contains("`r") -or -not $script:Call.HasStdin) {
    throw 'SSH stdin was not normalized and forwarded'
}
if (($script:Call.Arguments[-2..-1] -join '|') -ne 'root@test|sh -s') { throw 'SSH command changed' }
Invoke-Ssh -Cmd 'true' -User root -Remote test
if ($script:Call.HasStdin) { throw 'Command-only SSH unexpectedly redirects stdin' }
$script:Sshpass = '/fake/sshpass'
Invoke-Ssh -Cmd 'sh -s' -User root -Remote test -Pass test-only -Stdin $body
if ($script:Call.Exe -ne '/fake/sshpass' -or $script:Call.Stdin -cne $body) { throw 'sshpass lost stdin' }
Write-Host 'SSH stdin tests passed'
