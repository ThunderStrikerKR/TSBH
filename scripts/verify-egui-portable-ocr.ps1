param(
    [Parameter(Mandatory = $true)][string]$ZipPath,
    [Parameter(Mandatory = $true)][string]$HarnessPath
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
# Isolated CI only. Never start the GUI or select a business workspace.
if ($env:GITHUB_ACTIONS -ne "true" -or !$env:RUNNER_TEMP) {
    throw "Run this acceptance test only inside isolated GitHub Actions"
}
$root = Join-Path $env:RUNNER_TEMP ("반도허브-한글경로-" + [guid]::NewGuid().ToString("N"))
$rule = "BandoOcrAcceptance-" + [guid]::NewGuid().ToString("N")
$ruleCreated = $false
try {
    Expand-Archive -LiteralPath $ZipPath -DestinationPath $root
    $exe = Join-Path $root "bando-hub-egui.exe"
    $python = Join-Path $root "ocr/runtime/python.exe"
    $info = Get-Content -Raw -LiteralPath (Join-Path $root "build-info.json") | ConvertFrom-Json
    if ($info.commit -ne $env:GITHUB_SHA) { throw "Build provenance mismatch" }
    if ((Get-FileHash -LiteralPath $exe).Hash -ne $info.executableSha256) { throw "Extracted EXE hash mismatch" }
    $stream = [IO.File]::OpenRead($exe)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        if ($reader.ReadUInt16() -ne 0x5A4D) { throw "Missing DOS executable header" }
        $stream.Position = 0x3C
        $stream.Position = $reader.ReadUInt32()
        if ($reader.ReadUInt32() -ne 0x4550 -or $reader.ReadUInt16() -ne 0x8664) {
            throw "Not a Windows x64 PE executable"
        }
    } finally { $stream.Dispose() }
    if (!(Test-Path -LiteralPath $python -PathType Leaf)) { throw "Packaged Python missing" }
    # Fail closed unless an active outbound block is available.
    $profiles = @(Get-NetFirewallProfile)
    if ($profiles.Count -eq 0 -or @($profiles | Where-Object { $_.Enabled -ne "True" }).Count -gt 0) {
        throw "Windows Firewall is not enabled for all profiles"
    }
    New-NetFirewallRule -Name $rule -DisplayName $rule -Direction Outbound -Action Block -Program $python -Profile Any -Enabled True | Out-Null
    $ruleCreated = $true
    $active = Get-NetFirewallRule -PolicyStore ActiveStore -Name $rule
    if ($active.Enabled -ne "True" -or $active.Action -ne "Block" -or $active.Direction -ne "Outbound") {
        throw "Outbound Python firewall rule is not active"
    }
    Remove-Item Env:BANDO_OCR_RUNTIME_DIR,Env:PYTHONHOME,Env:PYTHONPATH -ErrorAction SilentlyContinue
    $sample = Join-Path $root "합성-발주서.png"
    $font = Join-Path $env:WINDIR "Fonts/malgun.ttf"
    if (!(Test-Path -LiteralPath $font)) { throw "Korean test font missing on Windows runner" }
    $sampleCode = @'
import sys
from PIL import Image, ImageDraw, ImageFont
im = Image.new("RGB", (1200, 280), "white")
draw = ImageDraw.Draw(im)
font = ImageFont.truetype(sys.argv[2], 80)
draw.text((40, 75), "\ubc18\ub3c4 709", font=font, fill="black")
im.save(sys.argv[1])
'@
    & $python -I -B -X utf8 -c $sampleCode $sample $font
    if ($LASTEXITCODE -ne 0) { throw "Synthetic PNG generation failed" }
    $harness = Join-Path $root "portable-ocr-test.exe"
    Copy-Item -LiteralPath $HarnessPath -Destination $harness
    $env:BANDO_CI_OCR_SYNTHETIC_IMAGE = $sample
    & $harness --exact cancelled_ocr_never_requires_runtime_or_starts_a_process --nocapture
    if ($LASTEXITCODE -ne 0) { throw "Native OCR cancellation guard failed" }
    & $harness --ignored --exact relocated_portable_ocr_recognizes_synthetic_image --nocapture
    if ($LASTEXITCODE -ne 0) { throw "Relocated native OCR inference failed" }
    Write-Host "PASS: verified extracted egui executable and native packaged OCR adapter (Python outbound blocked)."
} finally {
    if ($ruleCreated) { Remove-NetFirewallRule -Name $rule -ErrorAction Stop }
    Remove-Item Env:BANDO_CI_OCR_SYNTHETIC_IMAGE -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
