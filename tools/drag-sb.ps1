param(
    [string]$Md = "D:\20533\deskbox\fangwork\Scheda\test\big.md",
    [string]$Out = "$env:TEMP\drag.png",
    [int]$W = 1650, [int]$H = 1140,
    [int]$X = 1620, [int]$Y1 = 300, [int]$Y2 = 800, [int]$Wheel = 0
)
# 拖滚动条验收：按下滑块 → 移动 → 抬起 → 截图
Get-Process scheda -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
$env:SCHEDA_TRACE = "1"
$errf = "$env:TEMP\drag_err.txt"
$p = Start-Process -FilePath "D:\20533\deskbox\fangwork\Scheda\target\release\scheda.exe" -ArgumentList $Md -PassThru -RedirectStandardError $errf
Start-Sleep -Milliseconds 1500
Add-Type @"
using System; using System.Runtime.InteropServices;
public class D {
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int h2, uint f);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 public struct RECT { public int left, top, right, bottom; }
}
"@
[D]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$h = $p.MainWindowHandle
if ($W -gt 0) { [D]::SetWindowPos($h, [IntPtr](-1), 60, 20, $W, $H, 0x0040) | Out-Null }
[D]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 800
function LP([int]$x, [int]$y) { return [IntPtr]::new(($y -shl 16) -bor $x) }
if ($Wheel -ne 0) {
    for ($i = 0; $i -lt 6; $i++) { [D]::PostMessage($h, 0x020A, [IntPtr]::new(($Wheel -shl 16) -bor 0), (LP 800 600)) | Out-Null; Start-Sleep -Milliseconds 40 }
    Start-Sleep -Milliseconds 900
}
[D]::PostMessage($h, 0x0200, [IntPtr]0, (LP $X $Y1)) | Out-Null
Start-Sleep -Milliseconds 200
[D]::PostMessage($h, 0x0201, [IntPtr]1, (LP $X $Y1)) | Out-Null
Start-Sleep -Milliseconds 200
for ($y = $Y1; $y -le $Y2; $y += 40) {
    [D]::PostMessage($h, 0x0200, [IntPtr]1, (LP $X $y)) | Out-Null
    Start-Sleep -Milliseconds 60
}
[D]::PostMessage($h, 0x0202, [IntPtr]0, (LP $X $Y2)) | Out-Null
Start-Sleep -Milliseconds 900
Add-Type -AssemblyName System.Drawing
$r = New-Object D+RECT
[D]::GetWindowRect($h, [ref]$r) | Out-Null
$bmp = New-Object System.Drawing.Bitmap ($r.right - $r.left), ($r.bottom - $r.top)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [D]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"title=$((Get-Process scheda).MainWindowTitle) pw=$ok out=$Out"
