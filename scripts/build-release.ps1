# Builds both editions into release/:
#   LiteRay_<ver>_x64-setup.exe      - NSIS installer (per-user, no admin needed to install)
#   LiteRay_<ver>_x64-portable.zip   - LiteRay.exe + "portable" marker + xray/ folder
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

$version = (Get-Content (Join-Path $root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json).version
$core = Join-Path $root "src-tauri\resources\xray"
$out = Join-Path $root "release"

if (-not (Test-Path (Join-Path $core "xray.exe"))) {
    & (Join-Path $PSScriptRoot "fetch-core.ps1")
}

Push-Location $root
try {
    # tauri prints progress to stderr, so judge success by the exit code only.
    $ErrorActionPreference = "Continue"
    npx tauri build 2>&1 | ForEach-Object { "$_" }
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }
    $ErrorActionPreference = "Stop"

    New-Item -ItemType Directory -Force $out | Out-Null
    $target = Join-Path $root "src-tauri\target\release"

    $setup = Get-ChildItem (Join-Path $target "bundle\nsis") -Filter "*-setup.exe" | Sort-Object LastWriteTime | Select-Object -Last 1
    Copy-Item $setup.FullName (Join-Path $out $setup.Name) -Force

    $stage = Join-Path ([IO.Path]::GetTempPath()) ("literay-portable-" + [guid]::NewGuid())
    $dir = Join-Path $stage "LiteRay"
    New-Item -ItemType Directory -Force (Join-Path $dir "xray") | Out-Null
    Copy-Item (Join-Path $target "LiteRay.exe") $dir
    New-Item -ItemType File (Join-Path $dir "portable") | Out-Null
    Get-ChildItem $core -File | Where-Object Name -ne ".gitkeep" | Copy-Item -Destination (Join-Path $dir "xray")
    $zip = Join-Path $out "LiteRay_${version}_x64-portable.zip"
    if (Test-Path $zip) { Remove-Item $zip }
    Compress-Archive -Path $dir -DestinationPath $zip
    Remove-Item -Recurse -Force $stage

    Get-ChildItem $out | Select-Object Name, @{ n = "MB"; e = { [math]::Round($_.Length / 1MB, 1) } }
}
finally {
    Pop-Location
}
