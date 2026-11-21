param(
  [string]$Md = "D:\20533\deskbox\fangwork\Scheda\test\demo.md",
  [string]$Out = "$env:TEMP\scheda.png",
  [int]$W = 1150,
  [int]$H = 820,
  [int]$Scroll = 0,
  [int]$ClickX = -1,
  [int]$ClickY = -1
)
Get-Process scheda -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
$p = Start-Process -FilePath "D:\20533\deskbox\fangwork\Scheda\target\release\scheda.exe" -ArgumentList $Md -PassThru
Start-Sleep -Milliseconds 1500
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Shot {
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x,int y,int w,int h2,uint f);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr wp, IntPtr lp);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
 public struct RECT { public int left, top, right, bottom; }
}
"@
[Shot]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null
$h = $p.MainWindowHandle
if ($W -gt 0) {
  [Shot]::SetWindowPos($h, [IntPtr](-1), 80, 40, $W, $H, 0x0040) | Out-Null
}
[Shot]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 900
if ($Scroll -ne 0) {
  $lp = [IntPtr]::new((300 -shl 16) -bor 600)
  for ($i = 0; $i -lt 18; $i++) {
    [Shot]::PostMessage($h, 0x020A, [IntPtr]::new(($Scroll -shl 16) -bor 0), $lp) | Out-Null
    Start-Sleep -Milliseconds 30
  }
  Start-Sleep -Milliseconds 1100
}
if ($ClickX -ge 0) {
  $lp = [IntPtr]::new(($ClickY -shl 16) -bor $ClickX)
  [Shot]::PostMessage($h, 0x0201, [IntPtr]::new(1), $lp) | Out-Null
  Start-Sleep -Milliseconds 60
  [Shot]::PostMessage($h, 0x0202, [IntPtr]::zero, $lp) | Out-Null
  Start-Sleep -Milliseconds 1200
}
$r = New-Object Shot+RECT
[Shot]::GetWindowRect($h, [ref]$r) | Out-Null
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap ($r.right - $r.left), ($r.bottom - $r.top)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$okpw = [Shot]::PrintWindow($h, $hdc, 2)
$g.ReleaseHdc($hdc)
if (-not $okpw) {
  $g.CopyFromScreen($r.left, $r.top, 0, 0, $bmp.Size)
}
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"saved $Out pw=$okpw"

