param(
    [Parameter(Mandatory=$true)][string]$TargetDir
)
# Build only from independently vetted offline inputs. No automatic downloads.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "ocr-release-trust.ps1")
if (![Environment]::Is64BitOperatingSystem -or $env:OS -ne "Windows_NT") { throw "Windows x64 required." }
if (!$env:BANDO_OCR_INPUT_DIR -or !$env:BANDO_OCR_INPUT_MANIFEST -or !$env:BANDO_OCR_INPUT_SHA256) {
    throw "Approved offline OCR input directory, manifest and SHA256 are required."
}
$inputRoot = (Resolve-Path -LiteralPath $env:BANDO_OCR_INPUT_DIR).Path
$approval = (Resolve-Path -LiteralPath $env:BANDO_OCR_INPUT_MANIFEST).Path
Assert-ApprovedTree $inputRoot $approval $env:BANDO_OCR_INPUT_SHA256 @('python-embed.zip','pip.pyz','requirements.lock','wheels','models')
$target = [IO.Path]::GetFullPath((Join-Path (Get-Location).Path $TargetDir))
if (Test-Path -LiteralPath $target) { throw "Target exists; refusing to overwrite." }
$temp = Join-Path ([IO.Path]::GetTempPath()) ("bando-ocr-approved-" + [guid]::NewGuid().ToString("N"))
$inputs = Join-Path $temp "inputs"
$stage = Join-Path $temp "ocr"
$runtime = Join-Path $stage "runtime"
try {
    New-Item -ItemType Directory -Path $temp | Out-Null
    Copy-Item -LiteralPath $inputRoot -Destination $inputs -Recurse
    Assert-ApprovedTree $inputs $approval $env:BANDO_OCR_INPUT_SHA256 @('python-embed.zip','pip.pyz','requirements.lock','wheels','models')
    New-Item -ItemType Directory -Path $runtime -Force | Out-Null
    Expand-Archive -LiteralPath (Join-Path $inputs "python-embed.zip") -DestinationPath $runtime
    $python = Join-Path $runtime "python.exe"
    $pth = Join-Path $runtime "python311._pth"
    if (!(Test-Path -LiteralPath $python -PathType Leaf) -or !(Test-Path -LiteralPath $pth -PathType Leaf)) { throw "Invalid approved CPython 3.11 x64 archive." }
    $site = Join-Path $runtime "Lib\site-packages"
    New-Item -ItemType Directory -Path $site -Force | Out-Null
    @("python311.zip",".","Lib\site-packages","import site") | Set-Content -LiteralPath $pth -Encoding Ascii
    $requirements = Join-Path $inputs "requirements.lock"
    # Only exact pinned wheel requirements and SHA256 options are permitted.
    # The approved lock must include every transitive dependency, one requirement per line.
    $lines = @(Get-Content -LiteralPath $requirements | Where-Object { $_.Trim() -and !$_.Trim().StartsWith('#') })
    if (!$lines.Count) { throw "Empty OCR dependency lock." }
    foreach ($line in $lines) {
        if ($line.Trim() -notmatch '^[A-Za-z0-9][A-Za-z0-9_.-]*==[A-Za-z0-9][A-Za-z0-9_.+-]*(?:\s+--hash=sha256:[A-Fa-f0-9]{64})+$') {
            throw "OCR dependency lock must contain exact versions and wheel hashes only."
        }
    }
    $env:PYTHONDONTWRITEBYTECODE = "1"
    $env:PYTHONNOUSERSITE = "1"
    & $python "-I" "-B" "-X" "utf8" (Join-Path $inputs "pip.pyz") --isolated install --no-index --find-links (Join-Path $inputs "wheels") --require-hashes --only-binary=:all: --no-cache-dir --disable-pip-version-check --target $site -r $requirements
    if ($LASTEXITCODE -ne 0) { throw "Approved offline OCR dependency install failed." }
    Copy-Item -LiteralPath (Join-Path $inputs "models") -Destination (Join-Path $stage "models") -Recurse
    foreach ($name in @('PP-OCRv5_mobile_det','korean_PP-OCRv5_mobile_rec')) {
        foreach ($file in @('inference.json','inference.pdiparams','inference.yml')) {
            if (!(Test-Path -LiteralPath (Join-Path $stage ("models\" + $name + "\" + $file)) -PathType Leaf)) { throw "Missing approved OCR model file." }
        }
    }
    New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
    Move-Item -LiteralPath $stage -Destination $target
    Write-Host "[OCR] Candidate prepared. Independent release inventory approval still required."
} finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
