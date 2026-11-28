# Explicit native Windows x64 release packaging. Never launches the application or opens a business DB.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$release = Join-Path $repo "release"
$manifest = Join-Path $repo "egui-app\Cargo.toml"
$nonce = [guid]::NewGuid().ToString("N")
$stage = Join-Path $release ("egui-stage-" + $nonce)
$zipStage = Join-Path $release ("egui-stage-" + $nonce + ".zip")
$ocrStageRelative = "release/ocr-stage-" + $nonce
$ocrStage = Join-Path $repo $ocrStageRelative
$extracted = Join-Path ([IO.Path]::GetTempPath()) ("BANDO-HUB-" + [char]0xBC30 + [char]0xD3EC + "-" + $nonce)

function Test-OcrBundle([string]$Root) {
    if (!(Test-Path -LiteralPath (Join-Path $Root "runtime\python.exe") -PathType Leaf)) { return $false }
    foreach ($model in @("PP-OCRv5_mobile_det", "korean_PP-OCRv5_mobile_rec")) {
        foreach ($file in @("inference.json", "inference.pdiparams", "inference.yml")) {
            if (!(Test-Path -LiteralPath (Join-Path $Root ("models\" + $model + "\" + $file)) -PathType Leaf)) { return $false }
        }
    }
    return $true
}

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
    & cargo.exe build --release --manifest-path $manifest --bin bando-hub-egui --target x86_64-pc-windows-msvc --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) { throw "Rust release build failed (code=$LASTEXITCODE)." }
    $exe = Join-Path $targetDir "x86_64-pc-windows-msvc\release\bando-hub-egui.exe"
    if (!(Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Release EXE missing." }

    # Prefer an explicitly supplied OCR bundle, otherwise reuse existing local packages.
    $ocrSource = $null
    if ($env:BANDO_OCR_RUNTIME_DIR) {
        $ocrSource = (Resolve-Path -LiteralPath $env:BANDO_OCR_RUNTIME_DIR).Path
        if (!(Test-OcrBundle $ocrSource)) { throw "Explicit OCR runtime is incomplete." }
    } else {
        foreach ($candidate in @("release\ocr", "ocr", "src-tauri\target\release\ocr", "src-tauri\target\debug\ocr", "release\win-unpacked\ocr")) {
            $full = Join-Path $repo $candidate
            if (Test-OcrBundle $full) { $ocrSource = $full; break }
        }
        if (!$ocrSource) {
            Write-Host "[OCR] No local bundle found. Preparing existing supported offline OCR package..."
            & (Join-Path $PSScriptRoot "package-ocr-runtime.ps1") -TargetDir $ocrStageRelative
            if ($LASTEXITCODE -ne 0 -or !(Test-OcrBundle $ocrStage)) { throw "OCR bundle preparation failed." }
            $ocrSource = $ocrStage
        }
    }

    New-Item -ItemType Directory -Path $stage | Out-Null
    Copy-Item -LiteralPath $exe -Destination (Join-Path $stage "BANDO-HUB.exe")
    Copy-Item -LiteralPath $ocrSource -Destination (Join-Path $stage "ocr") -Recurse
    $imageSource = Join-Path $repo "public\product-images"
    $imageMap = Get-Content -LiteralPath (Join-Path $repo "src\tauri-product-images.json") -Raw | ConvertFrom-Json
    $imageNames = @($imageMap.PSObject.Properties | ForEach-Object { [string]$_.Value } | Sort-Object -Unique)
    if ($imageNames.Count -eq 0) { throw "Product image map is empty." }
    $imageStage = Join-Path $stage "product-images"
    New-Item -ItemType Directory -Path $imageStage | Out-Null
    foreach ($name in $imageNames) {
        if ($name -notmatch '^[a-zA-Z0-9_.-]+
    Copy-Item -LiteralPath (Join-Path $repo "scripts\RUN_BANDO_HUB.cmd") -Destination $stage
    @(
        "BANDO HUB v$version - Windows x64",
        "Extract the entire ZIP before use. Run BANDO-HUB.exe directly (RUN_BANDO_HUB.cmd is optional).",
        "Keep the ocr and product-images folders next to BANDO-HUB.exe.",
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
    foreach ($name in $imageNames) {
        $unpacked = Join-Path (Join-Path $extracted "product-images") $name
        if (!(Test-Path -LiteralPath $unpacked -PathType Leaf)) { throw "Extracted product image missing: $name" }
        if ((Get-FileHash -LiteralPath (Join-Path $imageSource $name) -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $unpacked -Algorithm SHA256).Hash) { throw "Extracted product image hash mismatch: $name" }
    }
    & (Join-Path $PSScriptRoot "verify-packaged-ocr.ps1") -OcrRoot (Join-Path $extracted "ocr")
    if ($LASTEXITCODE -ne 0) { throw "Extracted OCR verification failed." }
    # Save a newly prepared OCR cache only if there is no existing cache.
    $cache = Join-Path $release "ocr"
    if (($ocrSource -eq $ocrStage) -and !(Test-Path -LiteralPath $cache)) {
        Move-Item -LiteralPath $ocrStage -Destination $cache
    }

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
    foreach ($temp in @($stage, $zipStage, $ocrStage, $extracted)) {
        if (Test-Path -LiteralPath $temp) {
            Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
 -or $name -eq "." -or $name -eq "..") { throw "Invalid mapped product image name." }
        $source = Join-Path $imageSource $name
        if (!(Test-Path -LiteralPath $source -PathType Leaf) -or (Get-Item -LiteralPath $source).Length -eq 0) { throw "Missing mapped product image: $name" }
        Copy-Item -LiteralPath $source -Destination (Join-Path $imageStage $name)
    }
    Copy-Item -LiteralPath (Join-Path $repo "scripts\RUN_BANDO_HUB.cmd") -Destination $stage
    @(
        "BANDO HUB v$version - Windows x64",
        "Extract the entire ZIP before use. Run BANDO-HUB.exe directly (RUN_BANDO_HUB.cmd is optional).",
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
    & (Join-Path $PSScriptRoot "verify-packaged-ocr.ps1") -OcrRoot (Join-Path $extracted "ocr")
    if ($LASTEXITCODE -ne 0) { throw "Extracted OCR verification failed." }
    # Save a newly prepared OCR cache only if there is no existing cache.
    $cache = Join-Path $release "ocr"
    if (($ocrSource -eq $ocrStage) -and !(Test-Path -LiteralPath $cache)) {
        Move-Item -LiteralPath $ocrStage -Destination $cache
    }

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
    foreach ($temp in @($stage, $zipStage, $ocrStage, $extracted)) {
        if (Test-Path -LiteralPath $temp) {
            Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
