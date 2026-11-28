param(
    [Parameter(Mandatory=$true)][string]$LiveDir,
    [Parameter(Mandatory=$true)][string]$StageDir
)
# Legacy preparation entrypoint: never execute or approve an unknown OCR tree.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "ocr-release-trust.ps1")
if (!$env:BANDO_OCR_APPROVAL_MANIFEST -or !$env:BANDO_OCR_APPROVAL_SHA256) {
    throw "Independent release OCR approval is required."
}
$live = [IO.Path]::GetFullPath((Join-Path (Get-Location).Path $LiveDir))
$stage = [IO.Path]::GetFullPath((Join-Path (Get-Location).Path $StageDir))
$approval = (Resolve-Path -LiteralPath $env:BANDO_OCR_APPROVAL_MANIFEST).Path
if (Test-Path -LiteralPath $live) {
    Assert-ApprovedOcr $live $approval $env:BANDO_OCR_APPROVAL_SHA256
    Write-Host "[OCR] Approved cache verified."
    exit 0
}
& (Join-Path $PSScriptRoot "package-ocr-runtime.ps1") -TargetDir $StageDir
Assert-ApprovedOcr $stage $approval $env:BANDO_OCR_APPROVAL_SHA256
Write-Host "[OCR] Approved staged package verified."
