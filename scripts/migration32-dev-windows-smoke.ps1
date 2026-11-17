param([string]$OutDir = "dev-windows-smoke-evidence")
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
if ($env:GITHUB_ACTIONS -ne "true" -or -not $env:RUNNER_TEMP) {
  throw "CI only: never modify the user's live PC."
}
$repo = (Get-Location).Path
$out = Join-Path $repo $OutDir
New-Item -ItemType Directory -Force -Path $out | Out-Null
$appRoot = Join-Path $env:APPDATA "com.bandostand.bandohub.tauri"
$baseline = Join-Path $appRoot "bando-hub-v001.sqlite"
$testDb = Join-Path $appRoot "developer-test\bando-hub-test.sqlite"
$launcher = Join-Path $repo "DEV_WINDOWS_TAURI.cmd"
if (!(Test-Path $launcher)) { throw "Missing DEV_WINDOWS_TAURI.cmd" }
if ((Test-Path $baseline) -or (Test-Path $testDb)) { throw "Refusing existing SQLite database" }
New-Item -ItemType Directory -Force -Path $appRoot | Out-Null

# Build a CI-only synthetic baseline. No actual production SQLite is opened.
$seedScript = Join-Path $out "ci-create-seed.py"
@'
import sys, sqlite3, json
seed, path = sys.argv[1:]
with open(seed, encoding="utf-8") as f: state=json.load(f)
db=sqlite3.connect(path)
db.execute("CREATE TABLE app_state(id INTEGER PRIMARY KEY CHECK(id=1),payload TEXT NOT NULL)")
db.execute("INSERT INTO app_state(id,payload) VALUES (1,?)",(json.dumps(state,ensure_ascii=False),))
db.commit()
db.close()
'@ | Set-Content -LiteralPath $seedScript -Encoding utf8
& python $seedScript (Join-Path $repo "src-tauri\resources\state-seed.json") $baseline
if ($LASTEXITCODE -ne 0) { throw "Could not create synthetic CI SQLite baseline" }
$hashBefore = (Get-FileHash $baseline -Algorithm SHA256).Hash
$stdout = Join-Path $out "dev-stdout.txt"
$stderr = Join-Path $out "dev-stderr.txt"
$report = @{
  launcher="DEV_WINDOWS_TAURI.cmd"; sourceCommit=$env:GITHUB_SHA
  ciOnly=$true; releaseExeBuilt=$false; status="STARTED"
  testDb=$testDb
}
$env:BANDO_DEV_GUI_AUDIT = "true"
$env:VITE_BANDO_NATIVE_AUDIT = "true"
$env:BANDO_DEV_EVIDENCE = $out
$cmd = $null
try {
  $cmd = Start-Process -FilePath "cmd.exe" -ArgumentList @("/c","DEV_WINDOWS_TAURI.cmd") -WorkingDirectory $repo -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
  $native = $null
  $ready = $false
  for($i=0;$i -lt 1200;$i++) {
    Start-Sleep -Milliseconds 900
    if($cmd.HasExited) { throw "DEV launcher exited with $($cmd.ExitCode). See dev-stderr.txt" }
    $native = @(Get-Process -Name "bando_hub_tauri" -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$repo*" } | Select-Object -First 1)
    if((Test-Path $testDb) -and $native.Count -gt 0) { $ready=$true; break }
  }
  if(!$ready) { throw "DEV_WINDOWS_TAURI did not boot its native window and TEST SQLite" }
  Start-Sleep -Seconds 2
  $verify = Join-Path $out "ci-verify-test.py"
@'
import sys,sqlite3,json
path=sys.argv[1]
db=sqlite3.connect('file:'+path.replace('\\','/')+'?mode=ro',uri=True)
data=json.loads(db.execute("SELECT payload FROM app_state WHERE id=1").fetchone()[0])
# SQLite V2 stores catalog, client and order rows in relational tables.
# app_state intentionally no longer duplicates these JSON arrays.
catalog=db.execute("SELECT COUNT(*) FROM core_products").fetchone()[0]
orders=db.execute("SELECT COUNT(*) FROM core_orders").fetchone()[0]
clients=db.execute("SELECT COUNT(*) FROM core_clients").fetchone()[0]
assert catalog>0,"TEST relational catalog is missing"
assert orders==0,"TEST includes live order data"
assert clients==0,"TEST includes live client data"
assert isinstance(data.get("revision"),int),"TEST revision missing"
assert db.execute("SELECT 1 FROM sqlite_master WHERE name='hub_notifications'").fetchone()
print(json.dumps({"products":catalog,"orders":orders,"clients":clients,
 "revision":data["revision"],"notificationTable":True}))
db.close()
'@ | Set-Content -LiteralPath $verify -Encoding utf8
  $verified = (& python $verify $testDb) -join ""
  if($LASTEXITCODE -ne 0) { throw "Developer TEST SQLite content invalid" }
  $hashAfter = (Get-FileHash $baseline -Algorithm SHA256).Hash
  if($hashBefore -ne $hashAfter) { throw "Developer run modified the synthetic baseline" }
  $native = @(Get-Process -Name "bando_hub_tauri" -ErrorAction Stop | Where-Object { $_.Path -like "$repo*" } | Select-Object -First 1)
  if($native.Count -eq 0) { throw "Native Tauri exited" }
  $report.nativePid=$native[0].Id
  $report.windowTitle=$native[0].MainWindowTitle
  $report.testState=$verified | ConvertFrom-Json
  $report.baselineUnchanged=$true
  try {
    $desktop = & pwsh.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $repo "scripts\stage9-windows-desktop-probe.ps1") -ProcessId $native[0].Id -OutDir $out
    $report.desktop=$desktop | Out-String | ConvertFrom-Json
  } catch { $report.screenshotError=$_.Exception.Message }
  # DEV_WINDOWS_TAURI.cmd intentionally calls npm ci on first launch.
  # That purges --no-save packages installed before app startup, so install
  # Playwright only after this native developer process has opened.
  & npm.cmd install --no-save --no-package-lock --no-audit --no-fund playwright@1.55.1
  if($LASTEXITCODE -ne 0) { throw "Could not install real WebView2 inspection client after DEV npm ci" }
  $env:BANDO_DEV_NATIVE_PID = [string]$native[0].Id
  Write-Host "BANDO_DEV_BUSINESS_START with real Windows Tauri debug executable"
  & node scripts/migration32-dev-business-ui.mjs
  if($LASTEXITCODE -ne 0) { throw "Real DEV GUI business scenario test FAILED. See dev-business-results.json" }
  $report.businessScenarios="PASS"
  # The CI cleanup force-kills the launcher tree. To test saved geometry
  # properly, gracefully close the real DEV window and relaunch that EXE twice.
  $debugExe=Join-Path $repo "src-tauri\target\debug\bando_hub_tauri.exe"
  & pwsh.exe -NoProfile -ExecutionPolicy Bypass -File (
    Join-Path $repo "scripts\verify-dev-window-placement-win.ps1"
  ) -ExePath $debugExe -ExistingProcessId $native[0].Id
  if($LASTEXITCODE -ne 0) {
    throw "DEV window geometry/maximized restore across real EXE restarts failed"
  }
  $report.devWindowPlacement="PASS"
  $report.status="PASS"
  Write-Host ("BANDO_DEV_WINDOWS_SMOKE_PASS "+($report | ConvertTo-Json -Depth 8 -Compress))
} catch {
  $report.status="FAIL"
  $report.failure=$_.Exception.Message
  Write-Host ("BANDO_DEV_WINDOWS_SMOKE_FAIL "+$report.failure)
  throw
} finally {
  $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $out "dev-smoke-report.json") -Encoding utf8
  if($cmd -and !$cmd.HasExited) {
    & taskkill.exe /PID $cmd.Id /T /F 2>$null | Out-Null
  }
}
