# Shared functions for wakey scripts

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

function Invoke-Ext {
    param($Exe, $Arguments, $Label)
    $displayArgs = $Arguments.Clone()
    for ($i = 0; $i -lt $displayArgs.Count; $i++) {
        if ($displayArgs[$i] -eq '-pw' -and ($i + 1) -lt $displayArgs.Count) {
            $displayArgs[$i + 1] = '****'
        }
    }
    Write-Host ("[{0}] {1} {2}" -f $Label, $Exe, ($displayArgs -join ' ')) -ForegroundColor Cyan

    $out = & $Exe @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Error ("{0} exited with code {1}:`n{2}" -f $Label, $LASTEXITCODE, ($out -join "`n"))
        throw ("{0} exited with code {1}" -f $Label, $LASTEXITCODE)
    }
    return $out
}

function Invoke-Scp {
    param($Local, $Dest, $Pass, $HostKey, [switch]$Quiet, [switch]$Recurse)
    if ($pscp = Get-Command pscp.exe -ErrorAction SilentlyContinue) {
        $arguments = @('-scp')
        if ($Quiet) { $arguments += '-q' }
        if ($Recurse) { $arguments += '-r' }
        if ($HostKey) { $arguments += @('-batch', '-hostkey', $HostKey) }
        if ($Pass) { $arguments += @('-pw', $Pass) }
        $arguments += @($Local, $Dest)
        Invoke-Ext -Exe $pscp.Path -Arguments $arguments -Label 'scp'
    }
    else {
        $arguments = @('-O')
        if ($Quiet) { $arguments += '-q' }
        if ($Recurse) { $arguments += '-r' }
        $arguments += @($Local, $Dest)
        Invoke-Ext -Exe 'scp' -Arguments $arguments -Label 'scp'
    }
}

function Invoke-Ssh {
    param($Cmd, $User, $Remote, $Pass, [switch]$Quiet)
    $Cmd = Normalize-LineEndings $Cmd
    if ($plink = Get-Command plink.exe -ErrorAction SilentlyContinue) {
        $arguments = @('-batch', '-ssh')
        if ($Pass) { $arguments += @('-pw', $Pass) }
        $dest = if ($User) { "$User@$Remote" } else { $Remote }
        $arguments += $dest, $Cmd
        Invoke-Ext -Exe $plink.Path -Arguments $arguments -Label 'ssh'
    }
    else {
        $arguments = @()
        if ($Quiet) { $arguments += '-q' }
        $dest = if ($User) { "$User@$Remote" } else { $Remote }
        $arguments += $dest, $Cmd
        Invoke-Ext -Exe 'ssh' -Arguments $arguments -Label 'ssh'
    }
}
