# Build the desktop shell (GPUI, WebView2) for Windows and pack it:
#   dist/morph-<version>-x86_64-pc-windows-msvc.zip   morph.exe, Morph.ico, LICENSE.txt, README.md
#   dist/morph-<version>-x86_64-pc-windows-msvc.msi   per-machine installer (WiX v5, when `wix` is on PATH)
# Unsigned (docs/install-macos-windows.md has the SmartScreen steps). The
# WebView2 runtime ships with Windows 10 (21H2+) and 11.
#   powershell -ExecutionPolicy Bypass -File scripts/windows-dist.ps1   # uses $env:CARGO_TARGET_DIR or .\target
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }
$meta = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$version = ($meta.packages | Where-Object { $_.name -eq "morph-desktop" }).version
$name = "morph-$version-x86_64-pc-windows-msvc"

cargo build --locked --release -p morph-desktop
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$stage = Join-Path ([System.IO.Path]::GetTempPath()) ("morph-dist-" + [guid]::NewGuid())
$dir = Join-Path $stage $name
New-Item -ItemType Directory -Force -Path $dir | Out-Null
Copy-Item (Join-Path $targetDir "release\morph.exe") $dir
Copy-Item docs\brand\Morph.ico $dir
Copy-Item LICENSE (Join-Path $dir "LICENSE.txt")
Copy-Item README.md $dir

New-Item -ItemType Directory -Force -Path dist | Out-Null
$zip = "dist\$name.zip"
if (Test-Path $zip) { Remove-Item $zip }
# bsdtar (Windows 10+) writes "/" separators; Compress-Archive in Windows
# PowerShell 5 writes "\", which unzip and other tools reject.
tar.exe -a -c -f $zip -C $stage $name
if ($LASTEXITCODE -ne 0) { throw "tar failed" }
$outputs = @($zip)

if (Get-Command wix -ErrorAction SilentlyContinue) {
  # MSI versions are numbers: 0.2.0-alpha.8 installs as 0.2.0.8.
  $msiVersion = $version -replace '^(\d+\.\d+\.\d+)-(alpha|beta|rc)\.(\d+)$', '$1.$3'
  $msi = "dist\$name.msi"
  # The installer's screens come from WiX's UI extension.
  wix extension add -g WixToolset.UI.wixext/5.0.2
  if ($LASTEXITCODE -ne 0) { throw "wix extension add failed" }
  wix build packaging\windows\morph.wxs -arch x64 -ext WixToolset.UI.wixext -d "Version=$msiVersion" -d "Source=$dir" -o $msi
  if ($LASTEXITCODE -ne 0) { throw "wix build failed" }
  $outputs += $msi
} else {
  Write-Host "wix not on PATH: no MSI (dotnet tool install --global wix --version 5.0.2)"
}

$sums = foreach ($f in $outputs) { "{0}  {1}" -f (Get-FileHash -Algorithm SHA256 $f).Hash.ToLower(), (Split-Path -Leaf $f) }
$sums | Set-Content -Encoding ascii dist\SHA256SUMS-windows
Remove-Item -Recurse -Force $stage
$outputs
Get-Content dist\SHA256SUMS-windows
