param(
  [Parameter(Mandatory=$true)][string]$ExePath,
  [Parameter(Mandatory=$true)][int]$ExistingProcessId
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne "true" -or $env:RUNNER_OS -ne "Windows") {
  throw "Windows DEV window roundtrip is permitted only in isolated CI"
}
if (!(Test-Path -LiteralPath $ExePath)) { throw "Native debug EXE missing: $ExePath" }
if ($ExistingProcessId -le 0) { throw "Existing real Tauri DEV process is required" }
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class BandoDevWindowProbe {
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll", SetLastError=true)]
  public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
  [DllImport("user32.dll", SetLastError=true)]
  public static extern bool SetWindowPos(IntPtr hwnd, IntPtr after,
    int x, int y, int width, int height, uint flags);
  [DllImport("user32.dll", SetLastError=true)]
  public static extern bool PostMessage(IntPtr hwnd, uint msg, UIntPtr wp, IntPtr lp);
  [DllImport("user32.dll", SetLastError=true)]
  public static extern bool ShowWindow(IntPtr hwnd, int command);
  [DllImport("user32.dll", SetLastError=true)]
  public static extern bool IsZoomed(IntPtr hwnd);
}
'@
$appRoot=Join-Path $env:APPDATA "com.bandostand.bandohub.tauri"
$testStateFile=Join-Path $appRoot "window-state-developer.json"
$productionStateFile=Join-Path $appRoot "window-state.json"
$productionBefore=if(Test-Path -LiteralPath $productionStateFile){
  (Get-FileHash -LiteralPath $productionStateFile -Algorithm SHA256).Hash
} else { $null }
$instances=New-Object 'System.Collections.Generic.List[System.Diagnostics.Process]'
$vite=$null

function Get-WindowRect([System.Diagnostics.Process]$process) {
  $process.Refresh()
  if ($process.HasExited -or $process.MainWindowHandle -eq [IntPtr]::Zero) {
    throw "Native DEV HWND disappeared"
  }
  $rect=New-Object BandoDevWindowProbe+RECT
  if (![BandoDevWindowProbe]::GetWindowRect($process.MainWindowHandle,[ref]$rect)) {
    throw "GetWindowRect failed"
  }
  return @{x=$rect.Left; y=$rect.Top;
    width=$rect.Right-$rect.Left; height=$rect.Bottom-$rect.Top}
}
function Close-Gracefully([System.Diagnostics.Process]$process) {
  $process.Refresh()
  if ($process.HasExited) { throw "Native process exited before WM_CLOSE" }
  if (![BandoDevWindowProbe]::PostMessage($process.MainWindowHandle,
      [uint32]0x0010,[UIntPtr]::Zero,[IntPtr]::Zero)) { throw "WM_CLOSE failed" }
  if (!$process.WaitForExit(20000)) { throw "Native app ignored graceful close" }
  # Get-Process attaches to the existing Tauri child; its ExitCode can be null.
  # WaitForExit above proves shutdown; check a numerical code only if exposed.
  $exitCode=$process.ExitCode
  if ($null -ne $exitCode -and [int]$exitCode -ne 0) {
    throw "Native DEV exit code: $exitCode"
  }
  for($i=0;$i -lt 50 -and !(Test-Path -LiteralPath $testStateFile);$i++) {
    Start-Sleep -Milliseconds 100
  }
  if (!(Test-Path -LiteralPath $testStateFile)) {
    throw "Developer window state not saved on graceful exit"
  }
}
function Ensure-Vite {
  for($i=0;$i -lt 3;$i++) {
    try {
      $reply=Invoke-WebRequest "http://127.0.0.1:5173/" -UseBasicParsing -TimeoutSec 2
      if ($reply.StatusCode -eq 200) { return }
    } catch {}
    Start-Sleep -Milliseconds 200
  }
  if ($null -eq $script:vite) {
    $repo=Split-Path -Parent $PSScriptRoot
    $script:vite=Start-Process -FilePath "cmd.exe" -ArgumentList @(
      "/c","node_modules\.bin\vite.cmd --host 127.0.0.1 --strictPort"
    ) -WorkingDirectory $repo -WindowStyle Hidden -PassThru
  }
  for($i=0;$i -lt 100;$i++) {
    try {
      $reply=Invoke-WebRequest "http://127.0.0.1:5173/" -UseBasicParsing -TimeoutSec 2
      if ($reply.StatusCode -eq 200) { return }
    } catch {}
    Start-Sleep -Milliseconds 200
  }
  throw "Vite development frontend did not return after native close"
}
function Start-Again {
  Ensure-Vite
  $process=Start-Process -FilePath $ExePath -ArgumentList "--bando-developer-test" -PassThru
  $instances.Add($process)
  for($i=0;$i -lt 160;$i++) {
    Start-Sleep -Milliseconds 250
    $process.Refresh()
    if ($process.HasExited) { throw "Restarted DEV exe exited: $($process.ExitCode)" }
    if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
      Start-Sleep -Milliseconds 900
      return $process
    }
  }
  throw "Restarted DEV EXE did not create a native main window"
}

try {
  $first=Get-Process -Id $ExistingProcessId -ErrorAction Stop
  $instances.Add($first)
  $screen=[System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
  $width=[Math]::Max(1024,[Math]::Min(1300,$screen.Width-140))
  $height=[Math]::Max(700,[Math]::Min(825,$screen.Height-120))
  $x=$screen.Left+[Math]::Max(0,[Math]::Min(87,$screen.Width-$width))
  $y=$screen.Top+[Math]::Max(0,[Math]::Min(63,$screen.Height-$height))
  # Force restore first so a previous maximized window does not mask SetWindowPos.
  [BandoDevWindowProbe]::ShowWindow($first.MainWindowHandle,9) | Out-Null
  Start-Sleep -Milliseconds 250
  if (![BandoDevWindowProbe]::SetWindowPos($first.MainWindowHandle,
      [IntPtr]::Zero,$x,$y,$width,$height,[uint32]0x0014)) {
    throw "Cannot position the original DEV native window"
  }
  Start-Sleep -Milliseconds 1000
  $expected=Get-WindowRect $first
  if ([Math]::Abs($expected.x-$x) -gt 24 -or
      [Math]::Abs($expected.width-$width) -gt 24) {
    throw "Failed to adjust native DEV rect: $($expected | ConvertTo-Json -Compress)"
  }
  Close-Gracefully $first
  $saved=Get-Content -LiteralPath $testStateFile -Raw | ConvertFrom-Json
  if (!$saved.main) { throw "Developer saved state omitted the main window" }
  $second=Start-Again
  $restored=Get-WindowRect $second
  if ([Math]::Abs($expected.x-$restored.x) -gt 24 -or
      [Math]::Abs($expected.y-$restored.y) -gt 24 -or
      [Math]::Abs($expected.width-$restored.width) -gt 24 -or
      [Math]::Abs($expected.height-$restored.height) -gt 24) {
    throw "DEV restart geometry mismatch: first=$($expected | ConvertTo-Json -Compress) next=$($restored | ConvertTo-Json -Compress)"
  }
  Write-Host "DEV_WINDOW_RECT_RESTORED: $($restored | ConvertTo-Json -Compress)"
  # Native maximize exercises the plugin's maximized state even if DEV
  # WebView2 is still loading; Release titlebar click is separately audited.
  [BandoDevWindowProbe]::ShowWindow($second.MainWindowHandle,3) | Out-Null
  Start-Sleep -Milliseconds 850
  if (![BandoDevWindowProbe]::IsZoomed($second.MainWindowHandle)) {
    throw "Native DEV window failed to maximize"
  }
  Close-Gracefully $second
  $maximized=Get-Content -LiteralPath $testStateFile -Raw | ConvertFrom-Json
  if (!$maximized.main.maximized) { throw "DEV state did not save maximized=true" }
  $third=Start-Again
  if (![BandoDevWindowProbe]::IsZoomed($third.MainWindowHandle)) {
    $rect=Get-WindowRect $third
    Write-Host "DEV_WINDOW_MAXIMIZED_FAILURE_DIAGNOSTIC: saved=true rect=$($rect | ConvertTo-Json -Compress)"
    throw "DEV maximized window did not restore after restart"
  }
  Write-Host "DEV_WINDOW_MAXIMIZED_RESTORED: true"
  Close-Gracefully $third
  $productionAfter=if(Test-Path -LiteralPath $productionStateFile){
    (Get-FileHash -LiteralPath $productionStateFile -Algorithm SHA256).Hash
  } else { $null }
  if ($productionBefore -ne $productionAfter) {
    throw "Developer TEST window modified production window placement"
  }
  Write-Host "DEV_WINDOW_PRODUCTION_ISOLATION: true"
  Write-Host "DEV_WINDOW_RESTART_PASS: true"
} finally {
  foreach($process in $instances) {
    $process.Refresh()
    if(!$process.HasExited) {
      Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
    $process.Dispose()
  }
  if($null -ne $vite) {
    $vite.Refresh()
    if(!$vite.HasExited) {
      Stop-Process -Id $vite.Id -Force -ErrorAction SilentlyContinue
    }
    $vite.Dispose()
  }
}
