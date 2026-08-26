param(
  [Parameter(Mandatory = $true)][string]$Title,
  [Parameter(Mandatory = $true)][string]$OutputPath
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class WindowCaptureNative {
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr extraData);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint flags);
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
"@

$match = [IntPtr]::Zero
$callback = [WindowCaptureNative+EnumWindowsProc]{
  param([IntPtr]$hWnd, [IntPtr]$lParam)
  if (-not [WindowCaptureNative]::IsWindowVisible($hWnd)) { return $true }
  $text = New-Object System.Text.StringBuilder 512
  [void][WindowCaptureNative]::GetWindowText($hWnd, $text, $text.Capacity)
  if ($text.ToString() -eq $Title) {
    $script:match = $hWnd
    return $false
  }
  return $true
}
[void][WindowCaptureNative]::EnumWindows($callback, [IntPtr]::Zero)
if ($match -eq [IntPtr]::Zero) { throw "Window not found: $Title" }

$rect = New-Object WindowCaptureNative+RECT
[void][WindowCaptureNative]::GetWindowRect($match, [ref]$rect)
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
if ($width -le 0 -or $height -le 0) { throw "Invalid bounds for $Title" }

$directory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $directory | Out-Null
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$hdc = $graphics.GetHdc()
$printed = [WindowCaptureNative]::PrintWindow($match, $hdc, 2)
$graphics.ReleaseHdc($hdc)
if (-not $printed) { $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size) }
$bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
Write-Output $OutputPath
