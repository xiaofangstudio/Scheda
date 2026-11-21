param([int]$X = 620, [int]$Y = 758, [string]$Out = "$env:TEMP\click2.png", [string]$Md = " ", [int]$W = 1650, [int]$H = 1140)
Get-Process scheda -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
$p = Start-Process -FilePath "D:\20533\deskbox\fangwork\Scheda\target\release\scheda.exe" -ArgumentList "`"$Md`"" -PassThru
Start-Sleep -Milliseconds 1200
Add-Type @"
using System; using System.Runtime.InteropServices;
public class U {
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int w, int h2, uint f);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
 public struct RECT { public int left, top, right, bottom; }
}
"@
$h = $p.MainWindowHandle
if ($W -gt 0) { [U]::SetWindowPos($h, [IntPtr](-1), 60, 20, $W, $H, 0x0040) | Out-Null }
[U]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 700
$lp = [IntPtr]::new(($Y -shl 16) -bor $X)
[U]::PostMessage($h, 0x0200, [IntPtr]0, $lp) | Out-Null
Start-Sleep -Milliseconds 400
[U]::PostMessage($h, 0x0201, [IntPtr]1, $lp) | Out-Null
Start-Sleep -Milliseconds 80
[U]::PostMessage($h, 0x0202, [IntPtr]0, $lp) | Out-Null
Start-Sleep -Milliseconds 900
Add-Type -AssemblyName System.Drawing
$r = New-Object U+RECT
[U]::GetWindowRect($h, [ref]$r) | Out-Null
$bmp = New-Object System.Drawing.Bitmap ($r.right - $r.left), ($r.bottom - $r.top)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [U]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"title=$((Get-Process scheda).MainWindowTitle) pw=$ok out=$Out"
$p | Stop-Process -Force
