#!/usr/bin/env pwsh
# Run with pwsh -NoProfile -File scripts/tests/publish_preflight.ps1.
$ErrorActionPreference = 'Stop'
$publishScript = Join-Path $PSScriptRoot '../publish.ps1'
$tempDir = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
$calls = [Collections.Generic.List[string]]::new()

function cargo {
    $call = $args -join ' '
    $calls.Add($call)
    if ($args[0] -eq 'publish') { throw 'Unexpected publish' }
    $global:LASTEXITCODE = if ($call -eq $failCommand) { 7 } else { 0 }
}

function git { throw 'Unexpected git invocation' }

New-Item -ItemType Directory -Path $tempDir | Out-Null
Push-Location $tempDir
try {
    Set-Content Cargo.toml 'version = "0.1.0"'
    $fmt = 'fmt --all -- --check'
    $isolated = 'check --locked -p wakey-control-plane'
    $clippy = 'clippy --locked --workspace --all-targets --all-features -- -D warnings'
    $build = 'build --release --locked'
    $expected = @($fmt, $isolated, $clippy, $build)

    for ($i = 0; $i -lt $expected.Count; $i++) {
        $calls.Clear()
        $failCommand = $expected[$i]
        $failure = $null
        try {
            & $publishScript -Publish -Tag -RunnerMode none
        } catch {
            $failure = $_.Exception.Message
        }
        if ($failure -ne "cargo $failCommand failed (exit 7)" -or
            ($calls -join '|') -ne ($expected[0..$i] -join '|')) {
            throw "Release did not stop at $failCommand. Calls: $($calls -join ', '); error: $failure"
        }
    }

    $calls.Clear()
    $failCommand = ''
    & $publishScript -Version 0.2.0
    $expected[1] = 'check -p wakey-control-plane'
    if (($calls -join '|') -ne ($expected -join '|')) {
        throw "Explicit version did not allow lockfile refresh. Calls: $($calls -join ', ')"
    }
} finally {
    Pop-Location
    Remove-Item -LiteralPath $tempDir -Recurse
}

Write-Host 'Publish preflight tests passed'
