#!/usr/bin/env pwsh
# Test ipjs on remote ARM device

param(
    [string]$Package = "lda-ipjs",
    [switch]$AllPackages,
    [string]$BinaryFilter = "",
    [string]$Filter = "",
    [switch]$Exact,
    [switch]$List,
    [ValidateSet("debug", "release")]
    [string]$BuildProfile = "release",
    [string]$password,
    [switch]$Verbose,
    [string]$RemoteTestPath = "/tmp/tmp/wakey-test",
    [string]$RemoteHost = "root@192.168.100.1",
    [int]$RemotePort = 2222,
    [switch]$Ignored,
    [switch]$IncludeIgnored,
    [switch]$NoCapture,
    [switch]$ShowOutput,
    [int]$Threads = 0
)

$ErrorActionPreference = "Stop"

. "$PSScriptRoot/lib.ps1"

$password = Get-DefaultPassword $password

$sshRemote = $RemoteHost
$hostPort = Split-HostPort -Host $sshRemote -DefaultPort $RemotePort
$sshRemote = $hostPort.Host
$RemotePort = $hostPort.Port

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

    $paths = New-Object System.Collections.Generic.List[string]

    foreach ($line in $CargoOutput) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }

        try {
            $msg = $line | ConvertFrom-Json -ErrorAction Stop
        }
        catch {
            continue
        }

        if ($msg.reason -ne "compiler-artifact") {
            continue
        }

        if (-not $msg.executable) {
            continue
        }

        $isTestProfile = $false
        if ($null -ne $msg.profile -and $null -ne $msg.profile.test) {
            $isTestProfile = [bool]$msg.profile.test
        }
        if (-not $isTestProfile) {
            continue
        }

        $paths.Add([string]$msg.executable)
    }

    @($paths)
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

function New-RemoteRunDir {
    param(
        [string]$BasePath,
        [string]$PackageName
    )

    $baseNorm = Normalize-PosixPath $BasePath
    $leaf = [IO.Path]::GetFileName($baseNorm)
    $parent = Normalize-PosixPath ([IO.Path]::GetDirectoryName($baseNorm))
    if ([string]::IsNullOrWhiteSpace($parent)) {
        $parent = "/tmp"
    }
    if ([string]::IsNullOrWhiteSpace($leaf)) {
        $leaf = "wakey-test"
    }

    $random = [System.Guid]::NewGuid().ToString("N").Substring(0, 10)
    $safePackage = ($PackageName -replace '[^A-Za-z0-9._-]', '_')
    return (Join-PosixPath $parent "$leaf-$safePackage-$random")
}

function New-RemoteBinaryPath {
    param(
        [string]$RemoteRunDir,
        [System.IO.FileInfo]$TestBinary
    )

    $safeName = ($TestBinary.Name -replace '[^A-Za-z0-9._-]', '_')
    Join-PosixPath $RemoteRunDir $safeName
}

function Ensure-RemoteParentDir {
    param(
        [string]$RemoteDirPath,
        [string]$RemoteHost,
        [string]$Password,
        [int]$Port
    )

    $dir = Normalize-PosixPath $RemoteDirPath
    if ([string]::IsNullOrWhiteSpace($dir)) {
        return
    }
    Invoke-Ssh -Cmd ("mkdir -p " + (Quote-ShArg $dir)) -Remote $RemoteHost -Pass $Password -Port $Port -Quiet
}

$packages = if ($AllPackages) { Get-WorkspacePackages } else { @($Package) }
$failures = New-Object System.Collections.Generic.List[string]

foreach ($packageName in $packages) {
    Write-Host "Building tests for $packageName..." -ForegroundColor Cyan

    # Stream cargo output and convert to text
    $cargoOutput = cargo test --no-run -p $packageName --target armv7-unknown-linux-musleabihf --message-format json $(if ($BuildProfile -eq "release") { "-r" }) 2>&1 |
    ForEach-Object {
        $line = if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.ToString() } else { $_ }
        # if ($Verbose) {
        #     Write-Host $line # this thing fills QUICK im not printing any
        # }
        $line
    }

    if ($LASTEXITCODE -ne 0) {
        $failures.Add("$packageName (build failed)")
        Write-Warning "Build failed for $packageName"
        continue
    }

    $testBinaryPaths = Get-TestBinaryPaths $cargoOutput
    $testBinaries = @($testBinaryPaths | Get-Item)

    if ($BinaryFilter) {
        $testBinaries = @(
            $testBinaries |
            Where-Object {
                $_.Name -like "*$BinaryFilter*" -or $_.BaseName -like "*$BinaryFilter*"
            }
        )
    }

    if ($testBinaries.Count -eq 0) {
        $reason = if ($BinaryFilter) {
            "no test executables matched binary filter '$BinaryFilter'"
        } else {
            "no test executables"
        }
        Write-Host "Skipping ${packageName}: $reason" -ForegroundColor DarkYellow
        continue
    }

    Write-Host "Found $($testBinaries.Count) test $($testBinaries.Count -eq 1 ? "binary" : "binaries") for $packageName" -ForegroundColor Green

    $remoteRunDir = New-RemoteRunDir -BasePath $RemoteTestPath -PackageName $packageName
    Ensure-RemoteParentDir -RemoteDirPath $remoteRunDir -RemoteHost $sshRemote -Password $password -Port $RemotePort

    try {
        $remoteUploadDir = (Normalize-PosixPath $remoteRunDir).TrimEnd('/') + '/'
        
        # Convert local file paths to WSL paths if running in WSL
        $localFilePaths = @($testBinaries | ForEach-Object {
            if (Test-IsWsl) {
                ConvertTo-WslPath $_.FullName
            } else {
                $_.FullName
            }
        })
        
        Invoke-Scp -Local $localFilePaths -Dest "${sshRemote}:$remoteUploadDir" -Pass $password -Port $RemotePort -Quiet

        foreach ($testBinary in $testBinaries) {
            Write-Host "`nTesting: $packageName / $($testBinary.Name)" -ForegroundColor Cyan

            $remoteBinaryPath = New-RemoteBinaryPath -RemoteRunDir $remoteRunDir -TestBinary $testBinary

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
            $remoteCmd = Build-RemoteExecCommand -RemotePath $remoteBinaryPath -Arguments $parts

            # Run test binary (with chmod to ensure executable)
            try {
                Invoke-Ssh -Cmd $remoteCmd -Remote $sshRemote -Pass $password -Port $RemotePort
            }
            catch {
                $failures.Add("$packageName / $($testBinary.Name)")
                Write-Warning "Test binary failed: $packageName / $($testBinary.Name)"
                Write-Warning $_.Exception.Message
                continue
            }
        }
    }
    finally {
        try {
            Invoke-Ssh -Cmd ("rm -rf " + (Quote-ShArg $remoteRunDir)) -Remote $sshRemote -Pass $password -Port $RemotePort -Quiet
        }
        catch {
            Write-Warning "Failed to remove remote test dir: $remoteRunDir"
        }
    }
}

if ($failures.Count -gt 0) {
    Write-Error ("One or more test binaries failed: {0}" -f ($failures -join ", "))
    exit 1
}

Write-Host "`nDone!" -ForegroundColor Green
