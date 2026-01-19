#!/usr/bin/env pwsh
# Test ipjs on remote ARM device

param(
    [string]$Package = "lda-ipjs",
    [string]$TestName = "",
    [ValidateSet("debug", "release")]
    [string]$BuildProfile = "debug",
    [string]$password,
    [switch]$Verbose,
    [string]$RemoteTestPath = "/root/.bin/test",
    [string]$RemoteHost = "root@192.168.100.1"
)

$ErrorActionPreference = "Stop"

# Build tests and capture output
Write-Host "Building tests for $Package..." -ForegroundColor Cyan

# Stream cargo output anc convert to text
$cargoOutput = cargo test --no-run -p $Package --target armv7-unknown-linux-musleabihf $(if ($BuildProfile -eq "release") { "-r" }) 2>&1 |
ForEach-Object { 
    $line = if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.ToString() } else { $_ }
    if ($Verbose) {
        Write-Host $line
    }
    $line  # Pass through to capture
}

# Parse test binary paths from cargo output
$testBinaries = $cargoOutput | 
Select-String -Pattern "Executable.*\((.+)\)" | 
ForEach-Object { $_.Matches.Groups[1].Value } |
Get-Item

if ($testBinaries.Count -eq 0) {
    Write-Error "No test binaries found! Exiting..."
    # Write-Host "Cargo output:" -ForegroundColor Yellow
    # $cargoOutput | ForEach-Object { Write-Host $_ }
    exit 1
}

Write-Host "Found $($testBinaries.Count) test $($testBinaries.Count -eq 1 ? "binary" : "binaries")" -ForegroundColor Green

# Run each test binary
foreach ($testBinary in $testBinaries) {
    Write-Host "`nTesting: $($testBinary.Name)" -ForegroundColor Cyan
    
    # Copy to target
    pscp.exe -batch -scp -pw $password $testBinary.FullName ${RemoteHost}:$RemoteTestPath | Out-Null
    
    # Run on target
    $testArgs = "$(if ($Verbose) {"--nocapture --show-output"})"
    if ($TestName) {
        $testArgs = "$TestName $testArgs"
    }
    
    plink -batch -ssh $RemoteHost -pw $password "chmod +x $RemoteTestPath && $RemoteTestPath $testArgs"
}

Write-Host "`nDone!" -ForegroundColor Green
