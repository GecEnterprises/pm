# pm installer — Windows.
#
#   irm https://raw.githubusercontent.com/GecEnterprises/pm/trunk/install.ps1 | iex
#
# Downloads the latest (or -Version <tag>) pm.exe from GitHub Releases into
# %LOCALAPPDATA%\Programs\pm, verifies its SHA256, and adds that folder to your
# user PATH. No admin rights needed.

[CmdletBinding()]
param(
    [string] $Version = "latest"
)

$ErrorActionPreference = "Stop"
$repo = "GecEnterprises/pm"
$dest = Join-Path $env:LOCALAPPDATA "Programs\pm"
$headers = @{ "User-Agent" = "pm-install"; "Accept" = "application/vnd.github+json" }

$rel = if ($Version -eq "latest") {
    Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers $headers
} else {
    Invoke-RestMethod "https://api.github.com/repos/$repo/releases/tags/$Version" -Headers $headers
}

$exe = $rel.assets | Where-Object { $_.name -eq "pm.exe" } | Select-Object -First 1
if (-not $exe) { throw "release $($rel.tag_name) has no pm.exe asset" }
$shaAsset = $rel.assets | Where-Object { $_.name -eq "pm.exe.sha256" } | Select-Object -First 1

New-Item -ItemType Directory -Force -Path $dest | Out-Null
$target = Join-Path $dest "pm.exe"

# Can't overwrite a running pm.exe — stop it first.
Get-Process pm -ErrorAction SilentlyContinue | Stop-Process -Force

Write-Host "Downloading pm $($rel.tag_name)…"
$tmp = "$target.new"
Invoke-WebRequest $exe.browser_download_url -OutFile $tmp -Headers $headers

if ($shaAsset) {
    $want = (Invoke-WebRequest $shaAsset.browser_download_url -Headers $headers).Content.Trim().ToLower()
    $got = (Get-FileHash $tmp -Algorithm SHA256).Hash.ToLower()
    if ($want -and $want -ne $got) {
        Remove-Item $tmp -Force
        throw "checksum mismatch: expected $want, got $got"
    }
}
Move-Item $tmp $target -Force

# Add to user PATH if it isn't already there.
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($userPath -split ";") -notcontains $dest) {
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$dest", "User")
    Write-Host "Added $dest to your PATH — restart your shell to pick it up."
}

Write-Host "pm $($rel.tag_name) installed to $target"
