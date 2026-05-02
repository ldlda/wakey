#!/usr/bin/env pwsh
# this does one thing
param(
    [string]$PASSWD
)
. "$(Split-Path -Parent $PSScriptRoot)/scripts/lib.ps1"

$PASSWD = Get-DefaultPassword $PASSWD

Write-Output $PSScriptRoot

# idk what this does it works like that then thats how it is
pscp.exe -l root -scp -pw $PASSWD -r 192.168.100.1:/etc/ldlda_help $PSScriptRoot
pscp.exe -l root -scp -pw $PASSWD  192.168.100.1:/etc/rc.local $PSScriptRoot
pscp.exe -l root -scp -pw $PASSWD -r 192.168.100.1:/etc/wakey-agent $PSScriptRoot

Remove-Item -Recurse (Join-Path $PSScriptRoot "root")
pscp.exe -l root -scp -pw $PASSWD  -r 192.168.100.1:/root $PSScriptRoot


$initDir = Join-Path $PSScriptRoot 'init.d'
New-Item -ItemType Directory -Force -Path $initDir | Out-Null

# these are sum
$files = "update_wakey" , "wakey" , "update_tailscale" , "wireguard_setup", "lda-override"
$remote = $files.ForEach({ "/etc/init.d/$_" })
$remote | ForEach-Object {
    pscp.exe -l root -scp -pw $PASSWD "192.168.100.1:$_" "$initDir/"
}

$hotplugDir = Join-Path $PSScriptRoot 'hotplug.d'
New-Item -ItemType Directory -Force -Path $hotplugDir | Out-Null

$files = "dhcp/95-wakey", "neigh/95-wakey"

$files | ForEach-Object {
    New-Item -ItemType Directory -Force -Path (Split-Path "$hotplugDir/$_")
    pscp.exe -l root -scp -pw $PASSWD "192.168.100.1:/etc/hotplug.d/$_" "$hotplugDir/$_"
}
