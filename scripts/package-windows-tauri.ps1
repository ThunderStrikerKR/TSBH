# Explicit Windows distribution packaging. Not used by DEV_WINDOWS_TAURI.cmd.
# Builds the Tauri release EXE, stages an offline OCR bundle, then verifies a
# Korean-path extracted ZIP before publishing anything as the current release.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$release = Join-Path $repo "src-tauri\target\release"
$portable = Join-Path $release "BANDO-HUB-portable"
$zip = Join-Path $release "BANDO-HUB-portable.zip"
$nonce = [guid]::NewGuid().ToString("N")
$portableStage = Join-Path $release ("BANDO-HUB-portable-stage-" + $nonce)
$zipStage = Join-Path $release ("BANDO-HUB-portable-stage-" + $nonce + ".zip")
$extracted = Join-Path ([IO.Path]::GetTempPath()) ("반도허브-배포검증-" + $nonce)
$ocrStageRelative = "release/ocr-stage-" + $nonce
$ocrStage = Join-Path $repo ("release\ocr-stage-" + $nonce)
$ocrLive = Join-Path $repo "release\ocr"
$ocrBackup = Join-Path $repo ("release\ocr-backup-" + $nonce)
$zipBackup = Join-Path $release ("BANDO-HUB-portable-backup-" + $nonce + ".zip")
$portableBackup = Join-Path $release ("BANDO-HUB-portable-backup-" + $nonce)
$published = $false
Push-Location $repo
try {
    foreach ($tool in @("node.exe", "npm.cmd", "cargo.exe")) {
        if (!(Get-Command $tool -ErrorAction SilentlyContinue)) {
            throw "Missing Windows release build dependency: $tool"
        }
    }
    & (Join-Path $PSScriptRoot "ensure-node-deps.ps1")
    if ($LASTEXITCODE -ne 0) { throw "Node dependency installation failed" }

    Write-Host "[RELEASE] Compiling actual Tauri Windows EXE..."
    & npm.cmd run build:windows
    if ($LASTEXITCODE -ne 0) { throw "Tauri release EXE build failed (code=$LASTEXITCODE)" }
    $exe = Join-Path $release "bando_hub_tauri.exe"
    if (!(Test-Path -LiteralPath $exe)) { throw "Tauri release binary missing: $exe" }

    & (Join-Path $PSScriptRoot "ensure-ocr-package.ps1") -LiveDir "release/ocr" -StageDir $ocrStageRelative
    if ($LASTEXITCODE -ne 0) { throw "Offline OCR runtime preparation failed" }
    $ocrSource = if (Test-Path -LiteralPath $ocrStage) { $ocrStage } else { $ocrLive }
    foreach ($required in @(
        "runtime\python.exe",
        "models\PP-OCRv5_mobile_det\inference.json",
        "models\korean_PP-OCRv5_mobile_rec\inference.json",
        "ocr-build-fingerprint.txt"
    )) {
        if (!(Test-Path -LiteralPath (Join-Path $ocrSource $required))) {
            throw "Verified offline OCR package component missing: $required"
        }
    }

    New-Item -ItemType Directory -Path $portableStage -Force | Out-Null
    Copy-Item -LiteralPath $exe -Destination (Join-Path $portableStage "bando_hub.exe")
    Copy-Item -LiteralPath $ocrSource -Destination (Join-Path $portableStage "ocr") -Recurse
    Compress-Archive -Path (Join-Path $portableStage "*") -DestinationPath $zipStage

    # Verify actual relocated OCR and executable, never a source-tree-only
    # runtime. Even Japanese/Korean Windows usernames must not break OCR.
    New-Item -ItemType Directory -Path $extracted -Force | Out-Null
    Expand-Archive -LiteralPath $zipStage -DestinationPath $extracted
    if (!(Test-Path -LiteralPath (Join-Path $extracted "bando_hub.exe"))) {
        throw "Packaged EXE missing from extracted release ZIP"
    }
    & (Join-Path $PSScriptRoot "verify-packaged-ocr.ps1") -OcrRoot (Join-Path $extracted "ocr")
    if ($LASTEXITCODE -ne 0) { throw "Extracted offline OCR verification failed" }

    # Successful verification precedes replacing ANY previous published ZIP.
    # Keep the older release as backup during atomic-looking promotion.
    if (Test-Path -LiteralPath $zip) {
        Move-Item -LiteralPath $zip -Destination $zipBackup
    }
    Move-Item -LiteralPath $zipStage -Destination $zip
    if (Test-Path -LiteralPath $portable) {
        Move-Item -LiteralPath $portable -Destination $portableBackup
    }
    Move-Item -LiteralPath $portableStage -Destination $portable
    if (Test-Path -LiteralPath $ocrStage) {
        if (Test-Path -LiteralPath $ocrLive) {
            Move-Item -LiteralPath $ocrLive -Destination $ocrBackup
        }
        Move-Item -LiteralPath $ocrStage -Destination $ocrLive
    }
    $published = $true
    Write-Host "[RELEASE] Verified ZIP ready: $zip"
} catch {
    # A failed build never publishes a release ZIP. Restore any renamed ZIP.
    if ((Test-Path -LiteralPath $zipBackup) -and !(Test-Path -LiteralPath $zip)) {
        Move-Item -LiteralPath $zipBackup -Destination $zip
    }
    if ((Test-Path -LiteralPath $portableBackup) -and !(Test-Path -LiteralPath $portable)) {
        Move-Item -LiteralPath $portableBackup -Destination $portable
    }
    if ((Test-Path -LiteralPath $ocrBackup) -and !(Test-Path -LiteralPath $ocrLive)) {
        Move-Item -LiteralPath $ocrBackup -Destination $ocrLive
    }
    Write-Error ("Manual release packaging failed: " + $_.Exception.Message)
    exit 1
} finally {
    Pop-Location
    foreach ($temp in @($extracted, $zipStage, $portableStage)) {
        if (Test-Path -LiteralPath $temp) {
            Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    if ($published) {
        foreach ($old in @($zipBackup, $portableBackup, $ocrBackup)) {
            if (Test-Path -LiteralPath $old) {
                Remove-Item -LiteralPath $old -Recurse -Force -ErrorAction SilentlyContinue
            }
        }
    }
}
