param(
  [Parameter(Mandatory=$true)][string]$ExePath
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if ($env:GITHUB_ACTIONS -ne "true" -or $env:RUNNER_OS -ne "Windows") {
  throw "Real window-placement test may run only in an isolated Windows CI runner"
}
if (!(Test-Path -LiteralPath $ExePath)) { throw "EXE missing: $ExePath" }
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class BandoWindowPlacementProbe {
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

$saveDir=Join-Path $env:APPDATA "com.bandostand.bandohub.tauri"
$stateFile=Join-Path $saveDir "window-state-developer.json"
$prodFile=Join-Path $saveDir "window-state.json"
$prodBefore=if (Test-Path -LiteralPath $prodFile) {
  (Get-FileHash -LiteralPath $prodFile -Algorithm SHA256).Hash
} else { $null }
# Never delete the production layout; only reset the disposable CI test layout.
if (Test-Path -LiteralPath $stateFile) { Remove-Item -LiteralPath $stateFile -Force }
$env:WEBVIEW2_USER_DATA_FOLDER=Join-Path $env:RUNNER_TEMP "bando-window-position-webview2"
$instances=New-Object 'System.Collections.Generic.List[System.Diagnostics.Process]'
function Start-Bando {
  $process=Start-Process -FilePath $ExePath -ArgumentList "--bando-developer-test" -PassThru
  $instances.Add($process)
  for($i=0;$i -lt 160;$i++){
    Start-Sleep -Milliseconds 250
    $process.Refresh()
    if ($process.HasExited) { throw "EXE exited before showing window: $($process.ExitCode)" }
    if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
      # Restoration occurs as part of window creation. Let native window
      # placement events settle before measuring its actual physical rect.
      Start-Sleep -Milliseconds 850
      return $process
    }
  }
  throw "Timed out waiting for real Tauri main HWND"
}
function Get-Bounds([System.Diagnostics.Process]$p) {
  $p.Refresh()
  $rect=New-Object BandoWindowPlacementProbe+RECT
  if (![BandoWindowPlacementProbe]::GetWindowRect($p.MainWindowHandle,[ref]$rect)) {
    throw "Unable to read native Windows rectangle"
  }
  return @{ x=$rect.Left; y=$rect.Top;
    width=$rect.Right-$rect.Left; height=$rect.Bottom-$rect.Top }
}
function Close-Bando([System.Diagnostics.Process]$p) {
  $p.Refresh()
  if ($p.HasExited) { throw "Tauri app terminated unexpectedly before close" }
  if (![BandoWindowPlacementProbe]::PostMessage($p.MainWindowHandle,
      [uint32]0x0010,[UIntPtr]::Zero,[IntPtr]::Zero)) {
    throw "WM_CLOSE delivery failed"
  }
  if (!$p.WaitForExit(15000)) { throw "Tauri EXE ignored graceful WM_CLOSE" }
  if ($p.ExitCode -ne 0) { throw "Tauri EXE shutdown failed: $($p.ExitCode)" }
  for($i=0;$i -lt 40 -and !(Test-Path -LiteralPath $stateFile);$i++) {
    Start-Sleep -Milliseconds 100
  }
  if (!(Test-Path -LiteralPath $stateFile)) {
    throw "window-state plugin did not persist developer window placement"
  }
}

try {
  $screen=[System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
  $w=[Math]::Max(1024,[Math]::Min(1350,$screen.Width-120))
  $h=[Math]::Max(700,[Math]::Min(880,$screen.Height-100))
  $x=$screen.Left+[Math]::Max(0,[Math]::Min(75,$screen.Width-$w))
  $y=$screen.Top+[Math]::Max(0,[Math]::Min(55,$screen.Height-$h))
  $first=Start-Bando
  # SWP_NOZORDER | SWP_NOACTIVATE; real top-level HWND is moved/resized.
  if (![BandoWindowPlacementProbe]::SetWindowPos($first.MainWindowHandle,
      [IntPtr]::Zero,$x,$y,$w,$h,[uint32]0x0014)) {
    throw "Failed to resize/move the real Windows EXE"
  }
  Start-Sleep -Milliseconds 1000
  $expected=Get-Bounds $first
  if([Math]::Abs($expected.x-$x) -gt 24 -or
     [Math]::Abs($expected.width-$w) -gt 24) {
    throw "Window did not adopt requested native geometry: $($expected | ConvertTo-Json -Compress)"
  }
  Close-Bando $first
  $saved=Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
  if (!$saved.main) { throw "Saved state omitted the main window" }
  $second=Start-Bando
  $restored=Get-Bounds $second
  if ([Math]::Abs($restored.x-$expected.x) -gt 24 -or
      [Math]::Abs($restored.y-$expected.y) -gt 24 -or
      [Math]::Abs($restored.width-$expected.width) -gt 24 -or
      [Math]::Abs($restored.height-$expected.height) -gt 24) {
    throw "Native window placement differs after EXE restart: " +
      "saved=$($expected | ConvertTo-Json -Compress) restored=$($restored | ConvertTo-Json -Compress)"
  }
  Write-Host "WINDOW_PLACEMENT_RESTORED: $($restored | ConvertTo-Json -Compress)"
  # Maximize through the ACTUAL React titlebar button and Tauri's
  # windowControl("maximize") IPC, not a foreign Win32 ShowWindow shortcut.
  $root=[System.Windows.Automation.AutomationElement]::FromHandle(
    $second.MainWindowHandle)
  $condition=New-Object System.Windows.Automation.PropertyCondition(
    [System.Windows.Automation.AutomationElement]::NameProperty,
    "창 최대화 또는 복원")
  $button=$null
  for($i=0;$i -lt 100;$i++){
    $button=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
    if($null -ne $button){break}
    Start-Sleep -Milliseconds 150
  }
  if($null -eq $button) {throw "Actual app maximize button not found in Windows UI Automation"}
  $invoke=$button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
  $invoke.Invoke()
  Start-Sleep -Milliseconds 800
  if (![BandoWindowPlacementProbe]::IsZoomed($second.MainWindowHandle)) {
    throw "Actual application maximize button did not maximize native window"
  }
  Close-Bando $second
  $storedMax=Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
  Write-Host "WINDOW_MAXIMIZED_SAVED: $($storedMax.main.maximized)"
  if (!$storedMax.main.maximized) {
    throw "Window state file did not retain a maximized window on close"
  }
  $third=Start-Bando
  if (![BandoWindowPlacementProbe]::IsZoomed($third.MainWindowHandle)) {
    throw "Maximized window was not restored on reopening"
  }
  Write-Host "WINDOW_MAXIMIZED_RESTORED: true"
  Close-Bando $third
  $prodAfter=if (Test-Path -LiteralPath $prodFile) {
    (Get-FileHash -LiteralPath $prodFile -Algorithm SHA256).Hash
  } else { $null }
  if ($prodBefore -ne $prodAfter) {
    throw "Developer TEST window state modified production window placement"
  }
  Write-Host "WINDOW_PLACEMENT_TEST_ISOLATION: true"
} finally {
  foreach($p in $instances){
    $p.Refresh()
    if (!$p.HasExited) {
      # Only CI synthetic processes are forcibly cleaned after an error.
      Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    }
    $p.Dispose()
  }
}
