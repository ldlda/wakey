#!/usr/bin/env pwsh
# Test ipjs on remote ARM device

param(
    [string]$Package = "lda-ipjs",
    [switch]$AllPackages,
    [string]$Filter = "",
    [switch]$Exact,
    [switch]$List,
    [ValidateSet("debug", "release")]
    [string]$BuildProfile = "debug",
    [string]$password,
    [switch]$Verbose,
    [string]$RemoteTestPath = "/root/.bin/test",
    [string]$RemoteHost = "root@192.168.100.1",
    [switch]$Ignored,
    [switch]$IncludeIgnored,
    [switch]$NoCapture,
    [switch]$ShowOutput,
    [int]$Threads = 0
)

$ErrorActionPreference = "Stop"

. "$PSScriptRoot/lib.ps1"

$password = Get-DefaultPassword $password

function Get-WorkspacePackages {
    $metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    $workspaceMembers = [System.Collections.Generic.HashSet[string]]::new()
    foreach ($member in $metadata.workspace_members) {
        [void]$workspaceMembers.Add($member)
    }

    @(
        $metadata.packages |
        Where-Object { $workspaceMembers.Contains($_.id) } |
        Select-Object -ExpandProperty name
    )
}

function Get-TestBinaryPaths {
    param([string[]]$CargoOutput)

    @(
        $CargoOutput |
        Select-String -Pattern "Executable.*\((.+)\)" |
        ForEach-Object { $_.Matches.Groups[1].Value }
    )
}

function Build-RemoteExecCommand {
    param(
        [string]$RemotePath,
        [string[]]$Arguments
    )

    $quotedPath = Quote-ShArg $RemotePath
    $quotedArgs = @($Arguments | ForEach-Object { Quote-ShArg "$_" })
    $exec = (@($quotedPath) + $quotedArgs) -join " "
    "chmod +x $quotedPath && $exec"
}

$packages = if ($AllPackages) { Get-WorkspacePackages } else { @($Package) }
$failures = New-Object System.Collections.Generic.List[string]

foreach ($packageName in $packages) {
    Write-Host "Building tests for $packageName..." -ForegroundColor Cyan

    # Stream cargo output and convert to text
    $cargoOutput = cargo test --no-run -p $packageName --target armv7-unknown-linux-musleabihf $(if ($BuildProfile -eq "release") { "-r" }) 2>&1 |
    ForEach-Object {
        $line = if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.ToString() } else { $_ }
        if ($Verbose) {
            Write-Host $line
        }
        $line
    }

    if ($LASTEXITCODE -ne 0) {
        $failures.Add("$packageName (build failed)")
        Write-Warning "Build failed for $packageName"
        continue
    }

    $testBinaryPaths = Get-TestBinaryPaths $cargoOutput
    $testBinaries = @($testBinaryPaths | Get-Item)

    if ($testBinaries.Count -eq 0) {
        Write-Host "Skipping ${packageName}: no test executables" -ForegroundColor DarkYellow
        continue
    }

    Write-Host "Found $($testBinaries.Count) test $($testBinaries.Count -eq 1 ? "binary" : "binaries") for $packageName" -ForegroundColor Green

    foreach ($testBinary in $testBinaries) {
        Write-Host "`nTesting: $packageName / $($testBinary.Name)" -ForegroundColor Cyan

        try {
            # Copy and make executable
            Invoke-Scp -Local $testBinary.FullName -Dest "${RemoteHost}:$RemoteTestPath" -Pass $password -Quiet

            # Build test args
            $parts = @()
            if ($Filter) { $parts += $Filter }
            if ($Exact) { $parts += "--exact" }
            if ($List) { $parts += "--list" }
            if ($Ignored) { $parts += "--ignored" }
            if ($IncludeIgnored) { $parts += "--include-ignored" }
            if ($ShowOutput -or $Verbose) { $parts += "--show-output" }
            if ($NoCapture -or $Verbose) { $parts += "--nocapture" }
            if ($Threads -gt 0) { $parts += "--test-threads"; $parts += $Threads }
            $remoteCmd = Build-RemoteExecCommand -RemotePath $RemoteTestPath -Arguments $parts

            # Run test binary (with chmod to ensure executable)
            Invoke-Ssh -Cmd $remoteCmd -Remote $RemoteHost -Pass $password
        }
        catch {
            $failures.Add("$packageName / $($testBinary.Name)")
            Write-Warning "Test binary failed: $packageName / $($testBinary.Name)"
            Write-Warning $_.Exception.Message
            continue
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Error ("One or more test binaries failed: {0}" -f ($failures -join ", "))
    exit 1
}

Write-Host "`nDone!" -ForegroundColor Green
