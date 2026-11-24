# Bundles and installs ARZ-Excalibur strictly inside Adobe Premiere Pro's CEP extensions folder.
# Does NOT run any 24/7 Windows background process — Premiere Pro auto-starts & auto-stops it!
$ErrorActionPreference = "Stop"

# Stop any standalone instance if running so binary updates cleanly
Get-Process -Name "ARZ-Excalibur", "arz-excalibur" -ErrorAction SilentlyContinue | Stop-Process -Force

$src = Join-Path $PSScriptRoot "premiere-bridge\com.arz.excalibur.bridge"
$dstParent = Join-Path $env:APPDATA "Adobe\CEP\extensions"
$dst = Join-Path $dstParent "com.arz.excalibur.bridge"
$dstBin = Join-Path $dst "bin"
$dstAssets = Join-Path $dstBin "assets"

New-Item -ItemType Directory -Path $dstParent -Force | Out-Null
if (Test-Path $dst) {
    Remove-Item -Path $dst -Recurse -Force
}
Copy-Item -Path $src -Destination $dst -Recurse -Force
New-Item -ItemType Directory -Path $dstBin, $dstAssets -Force | Out-Null

$exeSrc = Join-Path $PSScriptRoot "target\release\arz-excalibur.exe"
if (-not (Test-Path $exeSrc)) {
    $exeSrc = Join-Path $PSScriptRoot "ARZ-Excalibur.exe"
}
Copy-Item -Path $exeSrc -Destination (Join-Path $dstBin "ARZ-Excalibur.exe") -Force

foreach ($cfg in @("settings.json", "macros.json")) {
    $cfgPath = Join-Path $PSScriptRoot $cfg
    if (Test-Path $cfgPath) {
        Copy-Item -Path $cfgPath -Destination (Join-Path $dstBin $cfg) -Force
    }
}
$wmPath = Join-Path $PSScriptRoot "assets\watermark.png"
if (Test-Path $wmPath) {
    Copy-Item -Path $wmPath -Destination (Join-Path $dstAssets "watermark.png") -Force
}

# Enable CEP extension loading across CSXS versions (including Premiere Pro 2027)
9..30 | ForEach-Object {
    $regPath = "HKCU:\Software\Adobe\CSXS.$_"
    if (-not (Test-Path $regPath)) { New-Item -Path $regPath -Force | Out-Null }
    Set-ItemProperty -Path $regPath -Name "PlayerDebugMode" -Value "1" -Type String
}

Write-Host "Installed! ARZ-Excalibur will now start ONLY when Adobe Premiere Pro opens, and close automatically when Premiere closes." -ForegroundColor Green
