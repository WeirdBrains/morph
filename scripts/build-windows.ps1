# Build Morph's Windows release and zip it next to the exe bundle.
#
# SQLCipher vendors OpenSSL. OpenSSL's nmake build compiles with cl, and the
# object paths sit under the Cargo target directory. flutter_rust_bridge_hooks
# puts that directory in TEMP (see its short-path workaround). The default
# user TEMP is long enough that those object paths pass the 260-character
# MAX_PATH and cl fails with C1083 ("Cannot open compiler generated file").
# This script points TEMP and TMP at a short directory for the build only.
#
# Requires, on PATH: flutter, cargo/rustup, perl (Strawberry Perl), nuget.exe
# (flutter_inappwebview_windows downloads the WebView2 SDK with it), and
# Visual Studio 2022 with the C++ desktop workload. The WebView2 Runtime is
# needed to run the app, not to build it.

param(
    [string]$Repo = "",
    [string]$ShortTemp = "C:\t\morph"
)

$ErrorActionPreference = "Stop"

if (-not $Repo) {
    $Repo = Split-Path -Parent $PSScriptRoot
}
$Repo = (Resolve-Path $Repo).Path

function Need([string]$Name) {
    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $cmd) {
        throw "Required program '$Name' is not on PATH."
    }
    Write-Output "$Name=$($cmd.Source)"
}

New-Item -ItemType Directory -Force -Path $ShortTemp | Out-Null
$env:TEMP = $ShortTemp
$env:TMP = $ShortTemp
$env:CI = "true"
$cmake = "C:\Program Files\CMake\bin"
if (Test-Path $cmake) {
    $env:Path = "$cmake;" + $env:Path
}

Write-Output "TEMP=$env:TEMP"
Need "flutter"
Need "cargo"
Need "perl"
Need "nuget"
Need "cmake"

$app = Join-Path $Repo "app"
Set-Location $app
& flutter config --enable-windows-desktop
if ($LASTEXITCODE -ne 0) { throw "flutter config failed: $LASTEXITCODE" }
& flutter pub get
if ($LASTEXITCODE -ne 0) { throw "flutter pub get failed: $LASTEXITCODE" }
& flutter build windows --release
if ($LASTEXITCODE -ne 0) { throw "flutter build windows failed: $LASTEXITCODE" }

$bundle = Join-Path $app "build\windows\x64\runner\Release"
if (-not (Test-Path (Join-Path $bundle "morph.exe"))) {
    throw "morph.exe was not produced at $bundle"
}

$version = "0.1.0"
$pubspec = Join-Path $app "pubspec.yaml"
foreach ($line in Get-Content $pubspec) {
    if ($line -match '^version:\s*([0-9]+\.[0-9]+\.[0-9]+)') {
        $version = $Matches[1]
        break
    }
}

$dist = Join-Path $Repo "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
$zip = Join-Path $dist "morph-$version-x86_64-pc-windows-msvc.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $bundle "*") -DestinationPath $zip
Write-Output "bundle=$bundle"
Write-Output "zip=$zip"
Write-Output ("zip_bytes=" + (Get-Item $zip).Length)
