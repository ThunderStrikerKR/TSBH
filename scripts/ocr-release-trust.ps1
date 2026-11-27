# Trust inputs must be obtained independently of the directory being verified.
# Never generate or approve an inventory during a release build.
function Assert-Sha256([string]$Path,[string]$Expected) {
    if ($Expected -notmatch '^[0-9a-fA-F]{64}$') { throw "Missing or invalid trusted SHA256." }
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Expected a regular, non-reparse file."
    }
    if ((Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash -ine $Expected) {
        throw "Trusted SHA256 mismatch: $($item.Name)"
    }
}

function Assert-ApprovedTree([string]$Root,[string]$Manifest,[string]$ManifestSha256,[string[]]$AllowedRoots) {
    Assert-Sha256 $Manifest $ManifestSha256
    $rootItem = Get-Item -LiteralPath $Root -Force -ErrorAction Stop
    if (!$rootItem.PSIsContainer) { throw "Approved tree is not a directory." }
    # Reject junctions/symlinks on the root and every ancestor.
    $ancestor = $rootItem
    while ($null -ne $ancestor) {
        if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse point in approved root path." }
        $ancestor = $ancestor.Parent
    }
    $prefix = $rootItem.FullName.TrimEnd('\') + '\'
    $manifestPath = (Get-Item -LiteralPath $Manifest -Force).FullName
    if ($manifestPath.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) {
        throw "Trust manifest must be stored outside the approved tree."
    }
    $document = Get-Content -LiteralPath $Manifest -Raw | ConvertFrom-Json
    if ($document.schemaVersion -ne 1 -or !$document.files) { throw "Invalid approval manifest." }
    $expected = @{}
    foreach ($entry in $document.files) {
        $relative = [string]$entry.path
        $hash = [string]$entry.sha256
        if (!$relative -or $relative -match '(^/|\\|:|(^|/)\.{1,2}(/|$)|//|/$)' -or $hash -notmatch '^[0-9a-fA-F]{64}$') {
            throw "Invalid approval manifest entry."
        }
        if ($expected.ContainsKey($relative)) { throw "Duplicate approval manifest entry." }
        $expected.Add($relative,$hash)
    }
    $pending = New-Object 'System.Collections.Generic.Stack[System.IO.DirectoryInfo]'
    $pending.Push($rootItem)
    $seen = @{}
    while ($pending.Count -gt 0) {
        $dir = $pending.Pop()
        foreach ($item in @(Get-ChildItem -LiteralPath $dir.FullName -Force -ErrorAction Stop)) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Reparse point in approved tree." }
            $relative = $item.FullName.Substring($prefix.Length).Replace('\','/')
            $top = ($relative -split '/',2)[0]
            if ($AllowedRoots -notcontains $top) { throw "Unexpected top-level package entry: $top" }
            if ($item.Name -match '(?i)^(?:\.env(?:\..*)?|.*\.(?:sqlite3?|db)(?:[-.].*)?|.*\.(?:bak|pfx|p12|key|xlsx|xlsm|csv))$') {
                throw "Private data or credential-like file in approved tree."
            }
            if ($item.PSIsContainer) { $pending.Push($item); continue }
            if (!$expected.ContainsKey($relative) -or $seen.ContainsKey($relative)) { throw "Unapproved package file: $relative" }
            Assert-Sha256 $item.FullName $expected[$relative]
            $seen.Add($relative,$true)
        }
    }
    if ($seen.Count -ne $expected.Count) { throw "Approved package file missing." }
}

function Assert-ApprovedOcr([string]$Root,[string]$Manifest,[string]$ManifestSha256) {
    Assert-ApprovedTree $Root $Manifest $ManifestSha256 @('runtime','models','bundle-info.json','ocr-build-fingerprint.txt')
    foreach ($file in @('runtime\python.exe','models\PP-OCRv5_mobile_det\inference.json','models\PP-OCRv5_mobile_det\inference.pdiparams','models\PP-OCRv5_mobile_det\inference.yml','models\korean_PP-OCRv5_mobile_rec\inference.json','models\korean_PP-OCRv5_mobile_rec\inference.pdiparams','models\korean_PP-OCRv5_mobile_rec\inference.yml')) {
        if (!(Test-Path -LiteralPath (Join-Path $Root $file) -PathType Leaf)) { throw "Required OCR component missing: $file" }
    }
}
