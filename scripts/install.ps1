# auraFX installer for Windows.
#   irm https://raw.githubusercontent.com/aashiquear/auraFX/main/scripts/install.ps1 | iex
# Downloads the latest installer from GitHub Releases and runs it for the current user.
param([string]$Version = "latest")
$ErrorActionPreference = "Stop"
$Repo = "aashiquear/auraFX"
$Api = if ($Version -eq "latest") { "https://api.github.com/repos/$Repo/releases/latest" } else { "https://api.github.com/repos/$Repo/releases/tags/$Version" }
$Release = Invoke-RestMethod $Api
$Asset = $Release.assets | Where-Object { $_.name -like "*-windows-x64-setup.exe" } | Select-Object -First 1
if (-not $Asset) { throw "No Windows installer in release $($Release.tag_name)" }
$Tmp = Join-Path $env:TEMP $Asset.name
Write-Host "auraFX: downloading $($Asset.browser_download_url)"
Invoke-WebRequest $Asset.browser_download_url -OutFile $Tmp
Start-Process -FilePath $Tmp -ArgumentList "/VERYSILENT", "/CURRENTUSER", "/NORESTART" -Wait
Remove-Item $Tmp
Write-Host "auraFX: installed. Start it from the Start menu."
