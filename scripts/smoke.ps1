param([switch]$Tray, [string]$ExePath)
$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class NativeWindow {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    public static extern IntPtr FindWindow(string className, string windowName);
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")]
    public static extern bool UpdateWindow(IntPtr window);
    [DllImport("user32.dll")]
    public static extern IntPtr GetDC(IntPtr window);
    [DllImport("user32.dll")]
    public static extern int ReleaseDC(IntPtr window, IntPtr dc);
    [DllImport("gdi32.dll")]
    public static extern uint GetPixel(IntPtr dc, int x, int y);
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
}
'@

$exe = if ($ExePath) { (Resolve-Path -LiteralPath $ExePath).Path } else { Join-Path $PSScriptRoot '..\target\debug\nightslide.exe' }
$env:NIGHTSLIDE_TEST_ISOLATED = '1'
if (-not $Tray) { $env:NIGHTSLIDE_TEST_NO_TRAY = '1' }
$process = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru
$second = $null
try {
    Start-Sleep -Milliseconds 700
    $hostWindow = [NativeWindow]::FindWindow('NightSlideHostSmoke', 'NightSlide')
    $popup = [NativeWindow]::FindWindow('NightSlidePopupSmoke', 'NightSlide')
    if ($hostWindow -eq [IntPtr]::Zero -or $popup -eq [IntPtr]::Zero) {
        throw "Tray host or popup window was not created. Host=$hostWindow Popup=$popup Exited=$($process.HasExited) ExitCode=$(if ($process.HasExited) { $process.ExitCode })"
    }
    $second = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru
    if (-not $second.WaitForExit(1500) -or $second.ExitCode -ne 0) {
        throw 'A second NightSlide instance remained running or failed.'
    }
    Start-Sleep -Milliseconds 150
    if ($process.HasExited) { throw 'The first instance stopped when the second started.' }
    if (-not [NativeWindow]::IsWindowVisible($popup)) { throw 'The second launch did not open the existing widget.' }
    [NativeWindow]::PostMessage($popup, 0x6, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 100
    [NativeWindow]::PostMessage($hostWindow, 0x8001, [UIntPtr]::Zero, [IntPtr]0x202) | Out-Null
    Start-Sleep -Milliseconds 350
    if (-not [NativeWindow]::IsWindowVisible($popup)) { throw 'Popup did not open.' }
    $rect = New-Object NativeWindow+Rect
    [NativeWindow]::GetWindowRect($popup, [ref]$rect) | Out-Null
    if (($rect.Right - $rect.Left) -ne 390 -or ($rect.Bottom - $rect.Top) -ne 74) {
        throw 'Popup has an unexpected size.'
    }
    [NativeWindow]::UpdateWindow($popup) | Out-Null
    $dc = [NativeWindow]::GetDC($popup)
    try {
        foreach ($corner in @(@(0, 0), @(389, 0), @(0, 73), @(389, 73))) {
            $color = [NativeWindow]::GetPixel($dc, $corner[0], $corner[1])
            if ($color -ne 0x00260e70) {
                throw "Widget corner $($corner[0]),$($corner[1]) has color $color instead of wine."
            }
        }
    } finally {
        [NativeWindow]::ReleaseDC($popup, $dc) | Out-Null
    }
    [NativeWindow]::PostMessage($popup, 0x100, [UIntPtr]::new([uint64]9), [IntPtr]::Zero) | Out-Null
    [NativeWindow]::PostMessage($popup, 0x6, [UIntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 150
    if ([NativeWindow]::IsWindowVisible($popup)) { throw 'Focus loss did not close the popup.' }
    [NativeWindow]::PostMessage($hostWindow, 0x8001, [UIntPtr]::Zero, [IntPtr]0x202) | Out-Null
    Start-Sleep -Milliseconds 150
    if (-not [NativeWindow]::IsWindowVisible($popup)) { throw 'Popup did not reopen.' }
    [NativeWindow]::PostMessage($popup, 0x100, [UIntPtr]::new([uint64]27), [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 150
    if ([NativeWindow]::IsWindowVisible($popup)) { throw 'Escape did not close the popup.' }
    [NativeWindow]::PostMessage($hostWindow, 0x111, [UIntPtr]::new([uint64]101), [IntPtr]::Zero) | Out-Null
    if (-not $process.WaitForExit(2000)) { throw 'Exit command did not stop the app.' }
    Write-Output 'Smoke check passed: one instance, open/reopen, size, square wine corners, Tab, focus close, Escape close, Exit.'
} finally {
    if ($second -and -not $second.HasExited) { Stop-Process -Id $second.Id -Force }
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    Remove-Item Env:NIGHTSLIDE_TEST_ISOLATED -ErrorAction SilentlyContinue
    Remove-Item Env:NIGHTSLIDE_TEST_NO_TRAY -ErrorAction SilentlyContinue
}
