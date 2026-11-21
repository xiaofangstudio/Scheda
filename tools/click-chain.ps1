param(
    [int[]]$Xs = @(1380, 411), [int[]]$Ys = @(36, 243),
    [string]$Out = "$env:TEMP\clickset.png", [string]$Md = " ", [int]$W = 1650, [int]$H = 1140
)
# 连点若干下再截图：用来验设置页里的开关
Get-Process scheda -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
$p = Start-Process -FilePath "D:\20533\deskbox\fangwork\Scheda\target\release\scheda.exe" -ArgumentList "`"$Md`"" -PassThru
Start-Sleep -Milliseconds 1200
Add-Type @"
using System; using System.Runtime.InteropServices;
public class U2 {
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int h2, uint f);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 public struct RECT { public int left, top, right, bottom; }
}
"@
$h = $p.MainWindowHandle
if ($W -gt 0) { [U2]::SetWindowPos($h, [IntPtr](-1), 60, 20, $W, $H, 0x0040) | Out-Null }
[U2]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 700
for ($i = 0; $i -lt $Xs.Count; $i++) {
    $lp = [IntPtr]::new(($Ys[$i] -shl 16) -bor $Xs[$i])
    [U2]::PostMessage($h, 0x0200, [IntPtr]0, $lp) | Out-Null
    Start-Sleep -Milliseconds 250
    [U2]::PostMessage($h, 0x0201, [IntPtr]1, $lp) | Out-Null
    Start-Sleep -Milliseconds 60
    [U2]::PostMessage($h, 0x0202, [IntPtr]0, $lp) | Out-Null
    Start-Sleep -Milliseconds 600
}
Start-Sleep -Milliseconds 600
Add-Type -AssemblyName System.Drawing
$r = New-Object U2+RECT
[U2]::GetWindowRect($h, [ref]$r) | Out-Null
$bmp = New-Object System.Drawing.Bitmap ($r.right - $r.left), ($r.bottom - $r.top)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [U2]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"title=$((Get-Process scheda).MainWindowTitle) pw=$ok out=$Out"
