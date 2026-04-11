# Shared functions for wakey scripts

# Platform detection globals
$script:IsWsl = $null
$script:WarnedNoSshpass = $false

function Test-IsWsl {
    if ($null -ne $script:IsWsl) {
        return $script:IsWsl
    }

    $script:IsWsl = $false

    # Check for WSL environment variables
    if ($env:WSL_DISTRO_NAME -or $env:WSL_INTEROP) {
        $script:IsWsl = $true
        return $true
    }

    # Check for /proc/version containing "microsoft" or "wsl"
    if ((Test-Path '/proc/version' -ErrorAction SilentlyContinue) -and 
        (Select-String -Path '/proc/version' -Pattern 'microsoft|wsl' -Quiet -ErrorAction SilentlyContinue)) {
        $script:IsWsl = $true
        return $true
    }

    return $false
}

function ConvertTo-WslPath {
    param([string]$WindowsPath)

    if ([string]::IsNullOrWhiteSpace($WindowsPath)) {
        return $WindowsPath
    }

    # If already a POSIX path, return as-is
    if ($WindowsPath -match '^/') {
        return $WindowsPath
    }

    # Handle Windows path (e.g., C:\path\to\file -> /mnt/c/path/to/file)
    if ($WindowsPath -match '^([A-Z]):(.*)$') {
        $drive = $matches[1].ToLower()
        $path = $matches[2] -replace '\\', '/'
        return "/mnt/$drive$path"
    }

    return $WindowsPath
}

function Get-DefaultPassword {
    param([string]$Password)

    if ($Password) {
        return $Password
    }

    $pwPath = Join-Path (Split-Path -Parent $PSScriptRoot) 'ar_data/pw'
    if (-not (Test-Path $pwPath)) {
        throw "Password not provided and default file not found: $pwPath"
    }

    return (Get-Content -Raw $pwPath).Trim()
}

function Normalize-LineEndings {
    param([string]$Text)
    if ($null -eq $Text) {
        return $null
    }
    return ($Text -replace "`r`n", "`n" -replace "`r", "")
}

function Quote-ShArg {
    param([AllowNull()][string]$Value)
    if ($null -eq $Value) {
        return "''"
    }
    return "'" + ($Value -replace "'", ("'" + '"' + "'" + '"' + "'")) + "'"
}

function Normalize-PosixPath {
    param([string]$Path)
    if ([string]::IsNullOrWhiteSpace($Path)) {
        return $Path
    }

    $normalized = $Path -replace '\\', '/'
    if ($normalized.Length -gt 1) {
        $normalized = $normalized -replace '/+', '/'
    }
    return $normalized
}

function Join-PosixPath {
    param(
        [string]$Left,
        [string]$Right
    )

    $leftNorm = Normalize-PosixPath $Left
    $rightNorm = Normalize-PosixPath $Right

    if ([string]::IsNullOrWhiteSpace($leftNorm)) {
        return $rightNorm
    }
    if ([string]::IsNullOrWhiteSpace($rightNorm)) {
        return $leftNorm
    }

    $leftTrim = $leftNorm.TrimEnd('/')
    $rightTrim = $rightNorm.TrimStart('/')
    return "$leftTrim/$rightTrim"
}

function Split-HostPort {
    param(
        [string]$HostName,
        [int]$DefaultPort = 22
    )

    $result = [ordered]@{
        Host = $HostName
        Port = $DefaultPort
    }

    if ([string]::IsNullOrWhiteSpace($HostName)) {
        return [PSCustomObject]$result
    }

    # IPv6 with brackets: [2001:db8::1]:2222
    if ($HostName -match '^\[(.+)\]:(\d+)$') {
        $result.Host = $matches[1]
        $result.Port = [int]$matches[2]
        return [PSCustomObject]$result
    }

    # host:port (single colon only, avoids plain IPv6 addresses)
    if ($HostName -match '^([^:]+):(\d+)$') {
        $result.Host = $matches[1]
        $result.Port = [int]$matches[2]
        return [PSCustomObject]$result
    }

    return [PSCustomObject]$result
}

function Get-SshpassCommand {
    if ($cmd = Get-Command sshpass -ErrorAction SilentlyContinue) {
        return $cmd.Path
    }
    return $null
}

function Invoke-Ext {
    param($Exe, $Arguments, $Label)
    $displayArgs = $Arguments.Clone()
    for ($i = 0; $i -lt $displayArgs.Count; $i++) {
        if ($displayArgs[$i] -eq '-pw' -and ($i + 1) -lt $displayArgs.Count) {
            $displayArgs[$i + 1] = '****'
        }
        if ($displayArgs[$i] -ceq '-p' -and ($i + 1) -lt $displayArgs.Count -and $Exe -like '*sshpass*') {
            $displayArgs[$i + 1] = '****'
        }
    }
    Write-Host ("[{0}] {1} {2}" -f $Label, $Exe, ($displayArgs -join ' ')) -ForegroundColor Cyan

    $out = & $Exe @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        if ($Exe -like '*sshpass*' -and $LASTEXITCODE -eq 6) {
            Write-Error ("{0} failed: sshpass could not confirm host key automatically (code 6). Use ssh once manually, or enable StrictHostKeyChecking=accept-new." -f $Label)
        }
        Write-Error ("{0} exited with code {1}:`n{2}" -f $Label, $LASTEXITCODE, ($out -join "`n"))
        throw ("{0} exited with code {1}" -f $Label, $LASTEXITCODE)
    }
    return $out
}

function Invoke-Scp {
    param($Local, $Dest, $Pass, $HostKey, [int]$Port = 22, [switch]$Quiet, [switch]$Recurse, [switch]$ForcePassword)
    
    $isWin = [bool]$IsWindows
    $isWsl = Test-IsWsl
    
    # Only use PuTTY on Windows (not in WSL)
    if ($isWin -and -not $isWsl -and ($pscp = Get-Command pscp.exe -ErrorAction SilentlyContinue)) {
        $arguments = @('-scp')
        if ($Quiet) { $arguments += '-q' }
        if ($Recurse) { $arguments += '-r' }
        if ($Port -gt 0) { $arguments += @('-P', $Port) }
        if ($HostKey) { $arguments += @('-batch', '-hostkey', $HostKey) }
        if ($Pass) { $arguments += @('-pw', $Pass) }
        $arguments += @($Local)
        $arguments += $Dest
        Invoke-Ext -Exe $pscp.Path -Arguments $arguments -Label 'scp'
    }
    else {
        # Convert Windows paths to WSL paths if needed
        if ($isWsl) {
            $Local = @($Local | ForEach-Object { ConvertTo-WslPath $_ })
            $Dest = ConvertTo-WslPath $Dest
        }
        
        $arguments = @('-O')
        if ($Quiet) { $arguments += '-q' }
        if ($Recurse) { $arguments += '-r' }
        # Avoid interactive host-key prompts (important for sshpass usage)
        $arguments += @('-o', 'StrictHostKeyChecking=accept-new')
        if ($ForcePassword) {
            $arguments += @('-o', 'PreferredAuthentications=keyboard-interactive,password')
            $arguments += @('-o', 'PubkeyAuthentication=no')
            $arguments += @('-o', 'KbdInteractiveAuthentication=yes')
            $arguments += @('-o', 'PasswordAuthentication=yes')
            $arguments += @('-o', 'NumberOfPasswordPrompts=1')
        }
        if ($Port -gt 0) { $arguments += @('-P', $Port) }
        $arguments += @($Local)
        $arguments += $Dest

        if ($Pass) {
            $sshpassExe = Get-SshpassCommand
            if ($sshpassExe) {
                $wrapped = @('-p', $Pass, 'scp') + $arguments
                Invoke-Ext -Exe $sshpassExe -Arguments $wrapped -Label 'scp'
                return
            }
            if (-not $script:WarnedNoSshpass) {
                Write-Warning 'Password was provided but sshpass is not installed; falling back to plain scp (key/agent auth expected).'
                $script:WarnedNoSshpass = $true
            }
        }

        if ($ForcePassword -and -not $Pass) {
            Write-Error 'ForcePassword requires Pass to be provided.'
            throw 'ForcePassword requires Pass'
        }

        if ($ForcePassword -and $Pass -and -not (Get-SshpassCommand)) {
            Write-Error 'ForcePassword was requested but sshpass is not installed. Install sshpass or disable ForcePassword.'
            throw 'ForcePassword requires sshpass'
        }

        Invoke-Ext -Exe 'scp' -Arguments $arguments -Label 'scp'
    }
}


function Invoke-Ssh {
    param($Cmd, $User, $Remote, $Pass, [int]$Port = 22, [switch]$Quiet, [switch]$ForcePassword)
    $Cmd = Normalize-LineEndings $Cmd
    
    $isWin = [bool]$IsWindows
    $isWsl = Test-IsWsl
    
    # Only use PuTTY on Windows (not in WSL)
    if ($isWin -and -not $isWsl -and ($plink = Get-Command plink.exe -ErrorAction SilentlyContinue)) {
        $arguments = @('-batch', '-ssh')
        if ($Port -gt 0) { $arguments += @('-P', $Port) }
        if ($Pass) { $arguments += @('-pw', $Pass) }
        $dest = if ($User) { "$User@$Remote" } else { $Remote }
        $arguments += $dest, $Cmd
        Invoke-Ext -Exe $plink.Path -Arguments $arguments -Label 'ssh'
    }
    else {
        $arguments = @()
        if ($Quiet) { $arguments += '-q' }
        # Avoid interactive host-key prompts (important for sshpass usage)
        $arguments += @('-o', 'StrictHostKeyChecking=accept-new')
        if ($ForcePassword) {
            $arguments += @('-o', 'PreferredAuthentications=keyboard-interactive,password')
            $arguments += @('-o', 'PubkeyAuthentication=no')
            $arguments += @('-o', 'KbdInteractiveAuthentication=yes')
            $arguments += @('-o', 'PasswordAuthentication=yes')
            $arguments += @('-o', 'NumberOfPasswordPrompts=1')
        }
        if ($Port -gt 0) { $arguments += @('-p', $Port) }
        $dest = if ($User) { "$User@$Remote" } else { $Remote }
        $arguments += $dest, $Cmd

        if ($Pass) {
            $sshpassExe = Get-SshpassCommand
            if ($sshpassExe) {
                $wrapped = @('-p', $Pass, 'ssh') + $arguments
                Invoke-Ext -Exe $sshpassExe -Arguments $wrapped -Label 'ssh'
                return
            }
            if (-not $script:WarnedNoSshpass) {
                Write-Warning 'Password was provided but sshpass is not installed; falling back to plain ssh (key/agent auth expected).'
                $script:WarnedNoSshpass = $true
            }
        }

        if ($ForcePassword -and -not $Pass) {
            Write-Error 'ForcePassword requires Pass to be provided.'
            throw 'ForcePassword requires Pass'
        }

        if ($ForcePassword -and $Pass -and -not (Get-SshpassCommand)) {
            Write-Error 'ForcePassword was requested but sshpass is not installed. Install sshpass or disable ForcePassword.'
            throw 'ForcePassword requires sshpass'
        }

        Invoke-Ext -Exe 'ssh' -Arguments $arguments -Label 'ssh'
    }
}

