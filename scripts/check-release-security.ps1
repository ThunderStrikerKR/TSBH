param(
    [Parameter(Mandatory=$true)][string]$OcrRoot,
    [Parameter(Mandatory=$true)][string]$ApprovalManifest,
    [Parameter(Mandatory=$true)][string]$ApprovalSha256
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "ocr-release-trust.ps1")
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $repo
try {
    foreach ($tool in @("git.exe","cargo.exe","cargo-audit.exe","gitleaks.exe","pip-audit.exe")) {
        if (!(Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Required security tool missing: $tool" }
    }
    $dirty = @(git.exe status --porcelain --untracked-files=normal)
    if ($LASTEXITCODE -ne 0 -or $dirty.Count -ne 0) { throw "Release requires a clean Git checkout." }
    & git.exe ls-files --error-unmatch egui-app/Cargo.lock | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Tracked egui Cargo.lock is required." }
    $shallow = (& git.exe rev-parse --is-shallow-repository)
    if ($LASTEXITCODE -ne 0 -or $shallow -ne "false") { throw "Full Git history is required for secret scanning." }
    & gitleaks.exe git --redact=100 --no-banner --log-opts="--all" .
    if ($LASTEXITCODE -ne 0) { throw "Git secret scan failed or found a leak." }
    foreach ($lock in @("egui-app/Cargo.lock","crates/bando-data-access/Cargo.lock")) {
        & git.exe ls-files --error-unmatch $lock | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Tracked Cargo.lock required: $lock" }
        & cargo.exe audit --file (Join-Path $repo $lock)
        if ($LASTEXITCODE -ne 0) { throw "Rust dependency audit failed: $lock" }
    }
    Assert-ApprovedOcr $OcrRoot $ApprovalManifest $ApprovalSha256
    # Inspect package metadata with a separately installed trusted audit tool;
    # never start the candidate OCR interpreter before integrity checks.
    & pip-audit.exe --path (Join-Path $OcrRoot "runtime\Lib\site-packages") --strict
    if ($LASTEXITCODE -ne 0) { throw "OCR dependency audit failed." }
    & (Join-Path $PSScriptRoot "test-ocr-release-trust.ps1")
    # Existing backup/restore regression tests create synthetic temporary DBs.
    & cargo.exe test --locked --manifest-path (Join-Path $repo "crates\bando-data-access\Cargo.toml") backup_service::tests
    if ($LASTEXITCODE -ne 0) { throw "Synthetic database backup and restore tests failed." }
    Write-Host "[SECURITY] Required release checks passed."
    exit 0
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
} finally {
    Pop-Location
}
