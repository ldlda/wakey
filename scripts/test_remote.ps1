#!/usr/bin/env pwsh
# Test ipjs on remote ARM device

param(
    [string]$Package = "lda-ipjs",
    [string]$TestName = "",
    [string]$BuildProfile = "debug",
    [string]$password,
    [switch]$Quiet

)

$ErrorActionPreference = "Stop"

# Build tests
Write-Host "Building tests for $Package..." -ForegroundColor Cyan
cargo test --no-run -p $Package --target armv7-unknown-linux-musleabihf $(if ($BuildProfile -eq "release") { "-r" })

# Find the test binary
$testBinary = Get-ChildItem -Path "target\armv7-unknown-linux-musleabihf\$BuildProfile\deps\test-*" -File | 
Where-Object { $_.Name -match '^test-[a-f0-9]+$' } |
Sort-Object LastWriteTime -Descending |
Select-Object -First 1

if (-not $testBinary) {
    Write-Error "Test binary not found!"
    exit 1
}

Write-Host "Found test binary: $($testBinary.Name)" -ForegroundColor Green

# Copy to target
Write-Host "Copying to target..." -ForegroundColor Cyan
pscp.exe -l root -batch -scp -pw $password $testBinary.FullName root@192.168.100.1:/tmp/test

# Run on target
Write-Host "Running tests on target..." -ForegroundColor Cyan
$testArgs = "$(if (!$Quiet) {" --nocapture --show-output"})"
if ($TestName) {
    $testArgs = "$TestName$testArgs"
}

plink -batch -ssh root@192.168.100.1 -pw $password "chmod +x /tmp/test && /tmp/test $testArgs"

Write-Host "Done!" -ForegroundColor Green
