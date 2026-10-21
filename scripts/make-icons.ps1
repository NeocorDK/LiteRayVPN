# Regenerates app and tray icons from the SVG sources in assets/.
$root = Split-Path -Parent $PSScriptRoot
$icons = Join-Path $root "src-tauri\icons"

function Invoke-TauriIcon([string[]]$arguments) {
    # tauri prints progress to stderr; only the exit code tells whether it failed.
    & npx tauri icon @arguments 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "tauri icon $arguments failed with exit code $LASTEXITCODE" }
}

Push-Location $root
try {
    Invoke-TauriIcon @("assets/logo.svg", "-o", $icons)
    foreach ($name in "tray-on", "tray-off") {
        $tmp = Join-Path ([IO.Path]::GetTempPath()) ("literay-icon-" + [guid]::NewGuid())
        Invoke-TauriIcon @("assets/$name.svg", "-o", $tmp, "-p", "32")
        Copy-Item (Join-Path $tmp "32x32.png") (Join-Path $icons "$name.png") -Force
        Remove-Item -Recurse -Force $tmp
    }
    # Mobile/Store variants are not used by a Windows-only build.
    Get-ChildItem $icons -Directory | Remove-Item -Recurse -Force
    Get-ChildItem $icons -Filter "Square*" | Remove-Item -Force
    Remove-Item (Join-Path $icons "StoreLogo.png"), (Join-Path $icons "icon.icns") -ErrorAction SilentlyContinue
}
finally {
    Pop-Location
}
