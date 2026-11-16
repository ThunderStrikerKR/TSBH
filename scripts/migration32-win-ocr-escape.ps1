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
$hwnd = $p.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { throw "Main Tauri window was not found for process $TargetPid" }
$rect = New-Object BandoOcrDesktop+Rect
$ok = [BandoOcrDesktop]::GetWindowRect($hwnd, [ref]$rect)
if (-not $ok) { throw "GetWindowRect failed" }
@{
  minimized = [BandoOcrDesktop]::IsIconic($hwnd)
  maximized = [BandoOcrDesktop]::IsZoomed($hwnd)
  rect = @($rect.Left, $rect.Top, $rect.Right, $rect.Bottom)
} | ConvertTo-Json -Compress
