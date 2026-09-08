<#
.SYNOPSIS
  Capture a single application window to a PNG.

.DESCRIPTION
  Development helper used to check the UI without installing the app. Uses
  PrintWindow with PW_RENDERFULLCONTENT, which captures GPU-composited surfaces
  such as WebView2 that a plain screen copy renders black.

.EXAMPLE
  powershell -File scripts/capture_window.ps1 -ProcessName universal-downloader -Out shot.png
#>
param(
    [Parameter(Mandatory = $true)][string]$ProcessName,
    [Parameter(Mandatory = $true)][string]$Out
)

Add-Type -AssemblyName System.Drawing

$signature = @'
using System;
using System.Runtime.InteropServices;

public static class Win32Capture {
    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdcBlt, uint nFlags);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }
}
'@

if (-not ("Win32Capture" -as [type])) {
    Add-Type -TypeDefinition $signature -Language CSharp
}

$process = Get-Process -Name $ProcessName -ErrorAction Stop |
    Where-Object { $_.MainWindowHandle -ne 0 } |
    Select-Object -First 1

if ($null -eq $process) { throw "no window found for process '$ProcessName'" }

$handle = $process.MainWindowHandle
[void][Win32Capture]::ShowWindow($handle, 9)   # SW_RESTORE
[void][Win32Capture]::SetForegroundWindow($handle)
Start-Sleep -Milliseconds 700

$rect = New-Object Win32Capture+RECT
[void][Win32Capture]::GetWindowRect($handle, [ref]$rect)
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
if ($width -le 0 -or $height -le 0) { throw "window has no visible area" }

$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$hdc = $graphics.GetHdc()
# 0x2 = PW_RENDERFULLCONTENT, required for WebView2 content.
[void][Win32Capture]::PrintWindow($handle, $hdc, 2)
$graphics.ReleaseHdc($hdc)
$graphics.Dispose()

$bitmap.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()
Write-Output "saved $Out ($width x $height)"
