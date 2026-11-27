# Explicit native Windows x64 release packaging. Never launches the application or opens a business DB.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$release = Join-Path $repo "release"
$manifest = Join-Path $repo "egui-app\Cargo.toml"
$nonce = [guid]::NewGuid().ToString("N")
$stage = Join-Path $release ("egui-stage-" + $nonce)
$zipStage = Join-Path $release ("egui-stage-" + $nonce + ".zip")
$extracted = Join-Path ([IO.Path]::GetTempPath()) ("BANDO-HUB-" + [char]0xBC30 + [char]0xD3EC + "-" + $nonce)

. (Join-Path $PSScriptRoot "ocr-release-trust.ps1")

Push-Location $repo
try {
    if (![Environment]::Is64BitOperatingSystem -or $env:OS -ne "Windows_NT") { throw "Windows x64 is required." }
    if (!(Get-Command cargo.exe -ErrorAction SilentlyContinue)) { throw "Rust Cargo is missing." }
    $packageSection = (Get-Content -LiteralPath $manifest -Raw) -split '(?m)^\[dependencies\]', 2
    $match = [regex]::Match($packageSection[0], '(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"\s*$')
    if (!$match.Success) { throw "Cannot determine the egui package version." }
    $version = $match.Groups[1].Value
    $zip = Join-Path $release ("BANDO-HUB-v" + $version + "-windows-x64.zip")
    New-Item -ItemType Directory -Path $release -Force | Out-Null
    $targetDir = Join-Path $repo "egui-app\target"
    Write-Host "[BUILD] BANDO HUB v$version (native egui, x64 Release)"
    if (!(Test-Path -LiteralPath (Join-Path $repo "egui-app\Cargo.lock") -PathType Leaf)) { throw "Committed egui Cargo.lock is required." }
    & git.exe ls-files --error-unmatch egui-app/Cargo.lock | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "egui Cargo.lock is not tracked." }
    & cargo.exe build --locked --release --manifest-path $manifest --bin bando-hub-egui --target x86_64-pc-windows-msvc --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) { throw "Rust release build failed (code=$LASTEXITCODE)." }
    $exe = Join-Path $targetDir "x86_64-pc-windows-msvc\release\bando-hub-egui.exe"
    if (!(Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Release EXE missing." }

    # Release packaging must never download or execute an unapproved runtime.
    if (!$env:BANDO_OCR_APPROVAL_MANIFEST -or !$env:BANDO_OCR_APPROVAL_SHA256) {
        throw "Independently approved OCR inventory and its SHA256 are required."
    }
    $approval = (Resolve-Path -LiteralPath $env:BANDO_OCR_APPROVAL_MANIFEST).Path
    $ocrSource = if ($env:BANDO_OCR_RUNTIME_DIR) {
        (Resolve-Path -LiteralPath $env:BANDO_OCR_RUNTIME_DIR).Path
    } else { Join-Path $release "ocr" }
    Assert-ApprovedOcr $ocrSource $approval $env:BANDO_OCR_APPROVAL_SHA256
    New-Item -ItemType Directory -Path $stage | Out-Null
    Copy-Item -LiteralPath $exe -Destination (Join-Path $stage "BANDO-HUB.exe")
    Copy-Item -LiteralPath $ocrSource -Destination (Join-Path $stage "ocr") -Recurse
    Assert-ApprovedOcr (Join-Path $stage "ocr") $approval $env:BANDO_OCR_APPROVAL_SHA256
    & (Join-Path $PSScriptRoot "check-release-security.ps1") -OcrRoot (Join-Path $stage "ocr") -ApprovalManifest $approval -ApprovalSha256 $env:BANDO_OCR_APPROVAL_SHA256
    if ($LASTEXITCODE -ne 0) { throw "Release security checks failed." }
    Copy-Item -LiteralPath (Join-Path $repo "scripts\RUN_BANDO_HUB.cmd") -Destination $stage
    @(
        "BANDO HUB v$version - Windows x64",
        "Extract the entire ZIP before use. Run RUN_BANDO_HUB.cmd.",
        "Keep the ocr folder next to BANDO-HUB.exe.",
        "Close the old app and back up existing business data before upgrading.",
        "This package does not contain, initialize or replace a business database.",
        "The app binds its existing standard production database; the developer TEST launcher is not included."
    ) | Set-Content -LiteralPath (Join-Path $stage "README.txt") -Encoding UTF8

    # Include all files (including hidden files), then verify the relocated ZIP.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory($stage, $zipStage)
    [IO.Compression.ZipFile]::ExtractToDirectory($zipStage, $extracted)
    $unpackedExe = Join-Path $extracted "BANDO-HUB.exe"
    if (!(Test-Path -LiteralPath (Join-Path $extracted "RUN_BANDO_HUB.cmd") -PathType Leaf)) { throw "Packaged launcher missing." }
    if (!(Test-Path -LiteralPath $unpackedExe -PathType Leaf)) { throw "Packaged EXE missing." }
    if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $unpackedExe -Algorithm SHA256).Hash) { throw "Extracted EXE hash mismatch." }
    Assert-ApprovedOcr (Join-Path $extracted "ocr") $approval $env:BANDO_OCR_APPROVAL_SHA256
    & (Join-Path $PSScriptRoot "verify-packaged-ocr.ps1") -OcrRoot (Join-Path $extracted "ocr")
    if ($LASTEXITCODE -ne 0) { throw "Extracted OCR verification failed." }
    # Publish only the verified archive; keep any previous same-version ZIP as backup.
    if (Test-Path -LiteralPath $zip) {
        $backup = $zip + "." + $nonce + ".bak"
        [IO.File]::Replace($zipStage, $zip, $backup)
    } else {
        [IO.File]::Move($zipStage, $zip)
    }
    Write-Host "[RELEASE] $zip"
    Write-Host "[SHA256] $((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash)"
    Write-Host "[NOTE] Windows application interaction and operational DB compatibility still require separate verification."
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
} finally {
    Pop-Location
    foreach ($temp in @($stage, $zipStage, $extracted)) {
        if (Test-Path -LiteralPath $temp) {
            Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
