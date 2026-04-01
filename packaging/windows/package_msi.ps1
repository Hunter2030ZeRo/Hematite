Param(
  [string]$Version = "0.1.0"
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "../..")
$BuildRoot = Join-Path $Root "build/package/windows"

Push-Location (Join-Path $Root "backend")
cargo build --release
Pop-Location

Push-Location (Join-Path $Root "frontend")
flutter pub get
flutter build windows --release
Pop-Location

$wix = Get-Command candle.exe -ErrorAction SilentlyContinue
if (-not $wix) {
  throw "WiX toolset is required. Install from https://wixtoolset.org/"
}

New-Item -ItemType Directory -Force -Path $BuildRoot | Out-Null
$bundleDir = Join-Path $Root "frontend/build/windows/x64/runner/Release"
Copy-Item (Join-Path $Root "backend/target/release/hematite-backend.exe") (Join-Path $bundleDir "hematite-backend.exe") -Force

$wxs = Join-Path $Root "packaging/windows/hematite.wxs"
& candle.exe -dVersion=$Version -out (Join-Path $BuildRoot "hematite.wixobj") $wxs
& light.exe -out (Join-Path $BuildRoot "hematite-$Version.msi") (Join-Path $BuildRoot "hematite.wixobj")

Write-Host "MSI package created: $BuildRoot/hematite-$Version.msi"
