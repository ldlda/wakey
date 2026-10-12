#!/usr/bin/env pwsh
# Run with pwsh -NoProfile -File scripts/tests/wsl_paths.ps1.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../lib.ps1"

# Intercept WSL so these tests never start or register a real runner.
$global:WakeyWslTest = @{
    Calls = [System.Collections.Generic.List[object]]::new()
    ExitCode = 0
    # Fake conversion response, chosen to exercise a path containing spaces.
    Output = '/mnt/c/runner home/config.yaml'
}
function global:wsl.exe {
    $global:WakeyWslTest.Calls.Add(@($args))
    $global:LASTEXITCODE = $global:WakeyWslTest.ExitCode
    if ($args[3] -eq 'wslpath') { $global:WakeyWslTest.Output }
}

foreach ($path in @('/home/lda/repo/scripts/helper.sh', '/mnt/c/runner home/config.yaml')) {
    $result = ConvertTo-WslPath -WindowsPath $path -Distro TestDistro
    if ($result -cne $path) { throw "POSIX path changed: $result" }
}
if ($global:WakeyWslTest.Calls.Count -ne 0) { throw 'POSIX paths invoked wslpath' }

foreach ($path in @('C:\runner home\config.yaml', 'C:/runner home/config.yaml', '\\server\share\runner home\config.yaml')) {
    $global:WakeyWslTest.Calls.Clear()
    $result = ConvertTo-WslPath -WindowsPath $path -Distro TestDistro
    if ($result -cne '/mnt/c/runner home/config.yaml') { throw 'Mock Windows conversion failed' }
    $call = $global:WakeyWslTest.Calls[0]
    if (($call[0..4] -join ' ') -cne '-d TestDistro -e wslpath -a' -or $call[5] -cne $path) {
        throw 'Distro or Windows path was not passed intact'
    }
}
if ((ConvertTo-WslPath 'D:\runner\config.yaml') -cne '/mnt/d/runner/config.yaml') {
    throw 'Existing drive conversion changed'
}
foreach ($path in @('root@router:/root/.bin/', 'relative/file')) {
    if ((ConvertTo-WslPath $path) -cne $path) { throw 'Existing SCP path handling changed' }
}

# Resolve missing relative paths from the PS location, including dot segments.
$temp = Join-Path ([System.IO.Path]::GetTempPath()) "wakey-wsl-paths-$([guid]::NewGuid())"
New-Item -ItemType Directory -Path $temp | Out-Null
Push-Location $temp
try {
    foreach ($relative in @('new folder/config.yaml', '.\new folder\config.yaml', 'nested/../new folder/config.yaml')) {
        $expected = Join-Path $temp 'new folder/config.yaml'
        $global:WakeyWslTest.Calls.Clear()
        $result = ConvertTo-WslPath -Path $relative -Distro TestDistro
        if ($IsWindows) {
            if ($global:WakeyWslTest.Calls[0][5] -cne $expected) { throw 'Relative Windows path was not resolved before conversion' }
        } elseif ($result -cne $expected -or $global:WakeyWslTest.Calls.Count -ne 0) {
            throw "Relative POSIX path was not resolved: $result"
        }
    }
} finally {
    Pop-Location
    Remove-Item -Recurse -Force $temp
}

foreach ($case in @(
    @{ ExitCode = 1; Output = '' },
    @{ ExitCode = 1; Output = '/mnt/c/bad' },
    @{ ExitCode = 0; Output = '' }
)) {
    $global:WakeyWslTest.ExitCode = $case.ExitCode
    $global:WakeyWslTest.Output = $case.Output
    $caught = $false
    try { ConvertTo-WslPath -WindowsPath 'C:\bad' -Distro TestDistro | Out-Null }
    catch { $caught = $_.Exception.Message -like 'wslpath *' }
    if (-not $caught) { throw 'Failed/empty wslpath result was accepted' }
}

if (-not $IsWindows) {
    $global:WakeyWslTest.Calls.Clear()
    $global:WakeyWslTest.ExitCode = 0
    & "$PSScriptRoot/../act_runner_wsl.ps1" -Action status -Distro TestDistro
    if ($global:WakeyWslTest.Calls.Count -ne 1 -or $global:WakeyWslTest.Calls[0][3] -ne 'bash') {
        throw 'POSIX wrapper paths should invoke only the runner command'
    }
    $command = $global:WakeyWslTest.Calls[0][5]
    if ($command -notlike '*act_runner_fedora.sh*' -or $command -notlike '*ar_data/wsl_runner/config.yaml*') {
        throw 'Wrapper did not resolve default paths'
    }
    $global:WakeyWslTest.Calls.Clear()
    $global:WakeyWslTest.Output = '/mnt/c/runner home/config.yaml'
    & "$PSScriptRoot/../act_runner_wsl.ps1" -Action status -Distro TestDistro `
        -RunnerHome 'new runner home' -RunnerBin 'bin/act_runner' -ConfigPath 'C:\runner home\config.yaml'
    if ($global:WakeyWslTest.Calls.Count -ne 2 -or $global:WakeyWslTest.Calls[0][3] -ne 'wslpath') {
        throw 'Explicit Windows config should be converted once before running bash'
    }
    $command = $global:WakeyWslTest.Calls[1][5]
    foreach ($path in @((Join-Path (Get-Location).Path 'new runner home'), (Join-Path (Get-Location).Path 'bin/act_runner'), '/mnt/c/runner home/config.yaml')) {
        if (-not $command.Contains((Quote-ShArg $path))) { throw "Explicit path missing from runner command: $path" }
    }
}
Write-Host 'WSL path tests passed'
