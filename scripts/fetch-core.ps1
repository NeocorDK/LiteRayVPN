# Downloads the Xray core, wintun.dll and geo files into src-tauri/resources/xray.
# Every download is verified against an official checksum before it is unpacked.
param(
    [string]$XrayVersion = "v26.9.9",
    [string]$WintunVersion = "0.14.1",
    [string]$WintunSha256 = "07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51",
    [string]$GeoRepo = "runetfreedom/russia-v2ray-rules-dat",
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root "src-tauri\resources\xray"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("literay-fetch-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force $dest, $tmp | Out-Null

function Get-Sha256([string]$path) { (Get-FileHash $path -Algorithm SHA256).Hash.ToLower() }

function Assert-Hash([string]$path, [string]$expected) {
    $actual = Get-Sha256 $path
    if ($actual -ne $expected.ToLower()) {
        throw "Checksum mismatch for $(Split-Path -Leaf $path): expected $expected, got $actual"
    }
}

try {
    # Xray core: zip + .dgst with "SHA2-256= <hash>" line
    if ($Force -or -not (Test-Path (Join-Path $dest "xray.exe"))) {
        $base = "https://github.com/XTLS/Xray-core/releases/download/$XrayVersion"
        $zip = Join-Path $tmp "xray.zip"
        Invoke-WebRequest "$base/Xray-windows-64.zip" -OutFile $zip
        $dgst = (Invoke-WebRequest "$base/Xray-windows-64.zip.dgst" -UseBasicParsing).Content
        if ($dgst -is [byte[]]) { $dgst = [Text.Encoding]::ASCII.GetString($dgst) }
        $line = ($dgst -split "`n") | Where-Object { $_ -match '^SHA2-256=' } | Select-Object -First 1
        if (-not $line) { throw "SHA2-256 line not found in .dgst" }
        Assert-Hash $zip ($line -replace '^SHA2-256=\s*', '').Trim()
        $unz = Join-Path $tmp "xray"
        Expand-Archive $zip $unz
        Copy-Item (Join-Path $unz "xray.exe") $dest -Force
        Set-Content (Join-Path $dest "VERSION") $XrayVersion -Encoding ascii
        Write-Host "xray $XrayVersion -> $dest"
    }

    # wintun.dll (amd64) from wintun.net, pinned hash from the official download page
    if ($Force -or -not (Test-Path (Join-Path $dest "wintun.dll"))) {
        $zip = Join-Path $tmp "wintun.zip"
        Invoke-WebRequest "https://www.wintun.net/builds/wintun-$WintunVersion.zip" -OutFile $zip
        Assert-Hash $zip $WintunSha256
        $unz = Join-Path $tmp "wintun"
        Expand-Archive $zip $unz
        Copy-Item (Join-Path $unz "wintun\bin\amd64\wintun.dll") $dest -Force
        Write-Host "wintun $WintunVersion -> $dest"
    }

    # geoip.dat / geosite.dat with .sha256sum files
    foreach ($name in "geoip.dat", "geosite.dat") {
        $target = Join-Path $dest $name
        if (-not $Force -and (Test-Path $target)) { continue }
        $base = "https://github.com/$GeoRepo/releases/latest/download"
        $file = Join-Path $tmp $name
        Invoke-WebRequest "$base/$name" -OutFile $file
        $sum = (Invoke-WebRequest "$base/$name.sha256sum" -UseBasicParsing).Content
        if ($sum -is [byte[]]) { $sum = [Text.Encoding]::ASCII.GetString($sum) }
        Assert-Hash $file (($sum.Trim() -split '\s+')[0])
        Copy-Item $file $target -Force
        Write-Host "$name -> $dest"
    }
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
