param(
  [Parameter(Mandatory=$true)][int]$TargetPid,
  [ValidateSet("state","escape")][string]$Action = "state"
)
$ErrorActionPreference = "Stop"
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class BandoOcrDesktop {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    public delegate bool WindowCallback(IntPtr hwnd, IntPtr lparam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(WindowCallback callback, IntPtr lparam);
    [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr hwnd, System.Text.StringBuilder title, int capacity);
    [DllImport("user32.dll")] public static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
}
'@
if ($Action -eq "escape") {
  [BandoOcrDesktop]::keybd_event(0x1B, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 180
  [BandoOcrDesktop]::keybd_event(0x1B, 0, 2, [UIntPtr]::Zero)
  Write-Output "ESC_SENT"
  exit 0
}
$p = Get-Process -Id $TargetPid -ErrorAction Stop
$candidates = [System.Collections.Generic.List[object]]::new()
$callback = [BandoOcrDesktop+WindowCallback]{
  param([IntPtr]$hwnd, [IntPtr]$unused)
  [uint32]$windowPid = 0
  [void][BandoOcrDesktop]::GetWindowThreadProcessId($hwnd, [ref]$windowPid)
  if ($windowPid -eq $TargetPid) {
    $title = [System.Text.StringBuilder]::new(512)
    [void][BandoOcrDesktop]::GetWindowText($hwnd, $title, $title.Capacity)
    $rect = New-Object BandoOcrDesktop+Rect
    [void][BandoOcrDesktop]::GetWindowRect($hwnd, [ref]$rect)
    $candidates.Add(@{
      handle = $hwnd.ToInt64()
      title = $title.ToString()
      visible = [BandoOcrDesktop]::IsWindowVisible($hwnd)
      minimized = [BandoOcrDesktop]::IsIconic($hwnd)
      maximized = [BandoOcrDesktop]::IsZoomed($hwnd)
      rect = @($rect.Left,$rect.Top,$rect.Right,$rect.Bottom)
    })
  }
  return $true
}
[void][BandoOcrDesktop]::EnumWindows($callback,[IntPtr]::Zero)
$match = @($candidates | Where-Object { $_.title -match "BANDO|반도" } | Select-Object -First 1)
if (-not $match) {
  $match = @($candidates | Where-Object { $_.visible } | Select-Object -First 1)
}
if (-not $match) {
  throw "No Tauri top-level window for PID $TargetPid. Found: $($candidates | ConvertTo-Json -Compress)"
}
$selected = $match[0]
@{
  minimized = $selected.minimized
  maximized = $selected.maximized
  rect = $selected.rect
  handle = $selected.handle
  title = $selected.title
  candidates = @($candidates.ToArray())
} | ConvertTo-Json -Compress -Depth 6
