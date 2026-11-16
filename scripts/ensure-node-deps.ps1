param()
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

# One source of truth for both daily dev and one-off release builds.
# Install Node packages only when package.json/lockfile, Node/npm version,
# or the installed toolchain has changed.
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location -LiteralPath $root

$lock = Join-Path $root "package-lock.json"
$manifest = Join-Path $root "package.json"
if (!(Test-Path -LiteralPath $lock) -or !(Test-Path -LiteralPath $manifest)) {
    throw "package.json / package-lock.json missing"
}
$binDir = Join-Path $root "node_modules\.bin"
$stamp = Join-Path $root "node_modules\.bando-deps-fingerprint.txt"
$tools = @("tauri.cmd", "vite.cmd", "tsc.cmd")
$nodeVersion = (& node --version).Trim()
if ($LASTEXITCODE -ne 0) { throw "Node.js version check failed" }
$npmVersion = (& npm.cmd --version).Trim()
if ($LASTEXITCODE -ne 0) { throw "npm version check failed" }
$inputs = @(
    "bando-node-deps-v1",
    (Get-FileHash -LiteralPath $lock -Algorithm SHA256).Hash,
    (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash,
    $nodeVersion,
    $npmVersion
)
$sha = [System.Security.Cryptography.SHA256]::Create()
try {
    $fingerprint = ([BitConverter]::ToString(
        $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($inputs -join "|")))
    )).Replace("-", "")
} finally {
    $sha.Dispose()
}
$complete = (Test-Path -LiteralPath $stamp)
if ($complete) {
    $complete = ((Get-Content -LiteralPath $stamp -Raw).Trim() -eq $fingerprint)
}
foreach ($tool in $tools) {
    if (!(Test-Path -LiteralPath (Join-Path $binDir $tool))) {
        $complete = $false
    }
}
if ($complete) {
    Write-Host "[Node] Lockfile and installed tools unchanged. Skipping npm installation."
    exit 0
}

Write-Host "[Node] First run or dependency/toolchain change detected. Running npm ci..."
# npm ci uses the checked-in lockfile and does not silently rewrite it.
& npm.cmd ci --no-audit --no-fund
if ($LASTEXITCODE -ne 0) { throw "npm ci failed (exit $LASTEXITCODE)" }
foreach ($tool in $tools) {
    if (!(Test-Path -LiteralPath (Join-Path $binDir $tool))) {
        throw "Required Node tool missing after install: $tool"
    }
}
Set-Content -LiteralPath $stamp -Value $fingerprint -Encoding Ascii
Write-Host "[Node] Dependency cache fingerprint recorded."
