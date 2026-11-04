# Build auraFX for Windows: portable zip and an Inno Setup installer, both with a bundled
# CPython + NumPy. Run on Windows (PowerShell 7) with Rust, and Inno Setup for the installer.
#   usage: pwsh scripts/package/windows.ps1 [-Out dist]
param([string]$Out = "dist")
$ErrorActionPreference = "Stop"
$Root = (Resolve-Path "$PSScriptRoot/../..").Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$Version = (Select-String -Path "$Root/Cargo.toml" -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$Work = "$Root/target/package/windows"
if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
New-Item -ItemType Directory -Force -Path $Work | Out-Null

# Portable Python (python-build-standalone), checksum verified.
$Tag = if ($env:PBS_TAG) { $env:PBS_TAG } else { "20261026" }
$Py = if ($env:PBS_PYTHON) { $env:PBS_PYTHON } else { "3.12.16" }
$Name = "cpython-$Py+$Tag-x86_64-pc-windows-msvc-install_only.tar.gz"
$Base = "https://github.com/astral-sh/python-build-standalone/releases/download/$Tag"
Invoke-WebRequest "$Base/$Name" -OutFile "$Work/$Name"
Invoke-WebRequest "$Base/SHA256SUMS" -OutFile "$Work/SHA256SUMS"
$Expected = (Select-String -Path "$Work/SHA256SUMS" -Pattern ([regex]::Escape($Name) + '$')).Line.Split(" ")[0]
$Actual = (Get-FileHash "$Work/$Name" -Algorithm SHA256).Hash.ToLower()
if ($Expected -ne $Actual) { throw "checksum mismatch for $Name" }
tar -xzf "$Work/$Name" -C $Work
$PyRoot = "$Work/python"
& "$PyRoot/python.exe" -m pip install --quiet --disable-pip-version-check --no-warn-script-location numpy

$env:PYO3_PYTHON = "$PyRoot/python.exe"
$env:CARGO_TARGET_DIR = "$Root/target/package/cargo-windows"
cargo build --release -p aurafx-app --manifest-path "$Root/Cargo.toml"
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

# Stage: aurafx.exe + python DLLs next to it + python\ (stdlib, site-packages).
$Stage = "$Work/auraFX"
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item "$env:CARGO_TARGET_DIR/release/aurafx.exe" $Stage
Copy-Item -Recurse $PyRoot "$Stage/python"
Copy-Item "$PyRoot/python3*.dll" $Stage
Get-ChildItem "$Stage/python" -Recurse -Directory -Include "test","tests","__pycache__","idlelib","tkinter","turtledemo" | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
Copy-Item "$Root/assets/icons/aurafx.ico", "$Root/LICENSE-MIT", "$Root/LICENSE-APACHE", "$Root/README.md" $Stage
& "$Stage/aurafx.exe" --version

Compress-Archive -Path "$Stage" -DestinationPath "$Out/auraFX-$Version-windows-x64.zip" -Force
$Iscc = Get-Command iscc -ErrorAction SilentlyContinue
if (-not $Iscc) { $Iscc = Get-Item "${env:ProgramFiles(x86)}/Inno Setup 6/ISCC.exe" -ErrorAction SilentlyContinue }
if ($Iscc) {
  & $Iscc.Source "/DAppVersion=$Version" "/DSourceDir=$Stage" "/DOutputDir=$Out" "$Root/packaging/windows/aurafx.iss"
  if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
} else {
  Write-Warning "Inno Setup not found: only the zip was built"
}
Write-Host "Built packages in $Out"
