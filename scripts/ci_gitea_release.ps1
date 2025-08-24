param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][string]$AssetPattern,
    [string]$ServerUrl,
    [string]$Token,
    [string]$Owner,
    [string]$Repo
)

$ErrorActionPreference = 'Stop'

# Resolve token
if (-not $Token) { $Token = $env:GITEA_TOKEN }
if (-not $Token) { throw 'Missing Gitea token. Set -Token or $env:GITEA_TOKEN' }

# Resolve repo owner/name
if (-not $Owner -or -not $Repo) {
    if ($env:GITHUB_REPOSITORY) {
        $parts = $env:GITHUB_REPOSITORY -split '/'
        $Owner = $Owner ?: $parts[0]
        $Repo = $Repo  ?: $parts[1]
    }
    else {
        throw 'Missing Owner/Repo and GITHUB_REPOSITORY not set'
    }
}

# Resolve API base
$api = $env:GITHUB_API_URL
if (-not $api -and $env:GITHUB_SERVER_URL) { $api = "$($env:GITHUB_SERVER_URL)/api/v1" }
if (-not $api -and $ServerUrl) { $api = "$ServerUrl/api/v1" }
if (-not $api) { throw 'Could not determine Gitea API URL. Provide -ServerUrl or set GITHUB_SERVER_URL/GITHUB_API_URL' }

# Find asset
$asset = Get-ChildItem -Path $AssetPattern -ErrorAction Stop | Select-Object -First 1
if (-not $asset) { throw "No asset matches pattern: $AssetPattern" }

$headers = @{ Authorization = "token $Token" }
$body = @{ tag_name = $Tag; name = $Tag; draft = $false; prerelease = $false } | ConvertTo-Json

Write-Host "Creating release $Tag for $Owner/$Repo"
try {
    $release = Invoke-RestMethod -Headers $headers -Uri "$api/repos/$Owner/$Repo/releases" -Method Post -Body $body -ContentType 'application/json'
}
catch {
    # If already exists, fetch by tag
    Write-Host "Create failed; trying to fetch existing release for tag $Tag" -ForegroundColor Yellow
    $release = Invoke-RestMethod -Headers $headers -Uri "$api/repos/$Owner/$Repo/releases/tags/$Tag" -Method Get
}

$rid = $release.id
if (-not $rid) { throw "Failed to resolve release id: $($release | ConvertTo-Json -Depth 5)" }

$uploadUri = "$api/repos/$Owner/$Repo/releases/$rid/assets?name=$($asset.Name)"
Write-Host "Uploading asset $($asset.Name)"
Invoke-WebRequest -Headers $headers -Uri $uploadUri -Method Post -InFile $asset.FullName -ContentType 'application/gzip' | Out-Null
Write-Host "Release published: tag $Tag with asset $($asset.Name)"
