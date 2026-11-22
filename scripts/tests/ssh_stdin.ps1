#!/usr/bin/env pwsh
#Requires -Version 7.4
# Run with pwsh -NoProfile -File scripts/tests/ssh_stdin.ps1 on Windows or Linux.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../lib.ps1"

# Read raw stdin in a child process. Text readers can hide a trailing CRLF.
$pwsh = (Get-Process -Id $PID).Path
# Keep -Command as a string: a script block makes PowerShell serialize stdin as CLIXML.
$readStdin = '$stream = [IO.MemoryStream]::new(); [Console]::OpenStandardInput().CopyTo($stream); [Convert]::ToBase64String($stream.ToArray())'
foreach ($inputText in @("line one`nline two`n", "caf$([char]0xe9)", '')) {
    $output = Invoke-Ext -Exe $pwsh -Arguments @('-NoProfile', '-Command', $readStdin) -Label test -Stdin $inputText
    $expected = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($inputText))
    if ([string]$output -cne $expected) { throw 'Native stdin bytes changed in transit' }
}

# Exercise the shell-facing behavior too where POSIX sh is available.
$body = "printf '%s\n' `"owner's directory`"`nprintf '%s\n' 'second line'"
if (-not $IsWindows) {
    $output = @(Invoke-Ext -Exe sh -Arguments @('-s') -Label test -Stdin $body)
    if (($output -join '|') -cne "owner's directory|second line") { throw 'stdin script changed in transit' }
}
$output = Invoke-Ext -Exe $pwsh -Arguments @('-NoProfile', '-Command', "'command-only'") -Label test
if ($output -cne 'command-only') { throw 'Command-only execution changed' }
$failed = $false
try {
    Invoke-Ext -Exe $pwsh -Arguments @('-NoProfile', '-Command', 'exit 7') -Label test -Stdin ''
} catch {
    $failed = $_.Exception.Message -like '*exited with code 7*'
}
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
