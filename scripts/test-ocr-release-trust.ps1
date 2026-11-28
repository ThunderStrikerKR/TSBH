# Synthetic only: no production paths, executables or customer data.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "ocr-release-trust.ps1")
$base = Join-Path ([IO.Path]::GetTempPath()) ("bando-trust-test-" + [guid]::NewGuid().ToString("N"))
$root = Join-Path $base "bundle"
$manifest = Join-Path $base "approved.json"
function Expect-Rejected([scriptblock]$Action,[string]$Case) {
    $rejected = $false
    try { & $Action } catch { $rejected = $true }
    if (!$rejected) { throw "Expected rejection: $Case" }
    Write-Host "[PASS] $Case"
}
try {
    New-Item -ItemType Directory -Path (Join-Path $root "runtime") -Force | Out-Null
    $file = Join-Path $root "runtime\fixture.bin"
    Set-Content -LiteralPath $file -Value "synthetic" -Encoding Ascii
    @{schemaVersion=1;files=@(@{path="runtime/fixture.bin";sha256=(Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash})} |
        ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $manifest -Encoding UTF8
    $digest = (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash
    Assert-ApprovedTree $root $manifest $digest @('runtime')
    Write-Host "[PASS] approved synthetic tree"
    Expect-Rejected { Assert-ApprovedTree $root $manifest ('0'*64) @('runtime') } "wrong approval hash"
    Set-Content -LiteralPath $file -Value "changed" -Encoding Ascii
    Expect-Rejected { Assert-ApprovedTree $root $manifest $digest @('runtime') } "modified file"
    Set-Content -LiteralPath $file -Value "synthetic" -Encoding Ascii
    $extra = Join-Path $root "runtime\extra.bin"
    Set-Content -LiteralPath $extra -Value "extra" -Encoding Ascii
    Expect-Rejected { Assert-ApprovedTree $root $manifest $digest @('runtime') } "unlisted file"
    Remove-Item -LiteralPath $extra
    Remove-Item -LiteralPath $file
    Expect-Rejected { Assert-ApprovedTree $root $manifest $digest @('runtime') } "missing approved file"
    Set-Content -LiteralPath $file -Value "synthetic" -Encoding Ascii
    $outside = Join-Path $base "outside"
    New-Item -ItemType Directory -Path $outside | Out-Null
    $junction = Join-Path $root "runtime\junction"
    New-Item -ItemType Junction -Path $junction -Target $outside | Out-Null
    try {
        Expect-Rejected { Assert-ApprovedTree $root $manifest $digest @('runtime') } "directory junction"
    } finally {
        [IO.Directory]::Delete($junction)
    }
    Assert-ApprovedTree $root $manifest $digest @('runtime')
    Write-Host "[PASS] all synthetic integrity checks"
} finally {
    Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction SilentlyContinue
}
