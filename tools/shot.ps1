param(
  [string]$Md = "D:\20533\deskbox\fangwork\Scheda\test\demo.md",
  [string]$Out = "$env:TEMP\scheda.png",
  [int]$W = 1150,
  [int]$H = 820,
  [int]$Scroll = 0,
  [int]$ClickX = -1,
  [int]$ClickY = -1,
  [int]$Copy = 0
)
Get-Process scheda -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
$env:SCHEDA_TRACE = '1'
$trace = "$env:TEMP\scheda-shot-trace.txt"
Remove-Item $trace -ErrorAction SilentlyContinue
$p = Start-Process -FilePath "D:\20533\deskbox\fangwork\Scheda\target\release\scheda.exe" -ArgumentList $Md -PassThru -RedirectStandardError $trace
Start-Sleep -Milliseconds 2400
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Shot {
 [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x,int y,int w,int h2,uint f);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr wp, IntPtr lp);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 public struct RECT { public int left, top, right, bottom; }
}
"@
# 注意：不要在这里改进程 DPI 上下文。本进程保持 DPI 无关时，
# SetWindowPos 的逻辑尺寸会按显示器缩放 ×1.5 落到真实窗口，窗口尺寸与 paint 日志一致。
$h = $p.MainWindowHandle
if ($W -gt 0) {
  [Shot]::SetWindowPos($h, [IntPtr](-2), 80, 40, $W, $H, 0x0040) | Out-Null
  Start-Sleep -Milliseconds 400
  # 再落一次：首次搬家可能伴随 DPI/尺寸校正，第二次才是最终尺寸
  [Shot]::SetWindowPos($h, [IntPtr](-2), 80, 40, $W, $H, 0x0040) | Out-Null
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
# 真实客户区像素尺寸以 App 自己 paint 日志为准（本进程 DPI 上下文读到的矩形可能被虚拟化）
Start-Sleep -Milliseconds 200
$pw = 0; $ph = 0
if (Test-Path $trace) {
  $last = Get-Content $trace | Where-Object { $_ -match 'paint w=(\d+) h=(\d+)' } | Select-Object -Last 1
  if ($last -match 'paint w=(\d+) h=(\d+)') { $pw = [int]$Matches[1]; $ph = [int]$Matches[2] }
}
if ($pw -lt 100 -or $ph -lt 100) {
  $dpi = [Shot]::GetDpiForWindow($h)
  if ($dpi -eq 0) { $dpi = 96 }
  $k = $dpi / 96.0
  $pw = [int](($r.right - $r.left) * $k)
  $ph = [int](($r.bottom - $r.top) * $k)
}
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap $pw, $ph
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$okpw = [Shot]::PrintWindow($h, $hdc, 2)
if ((-not $okpw) -or ($Copy -ne 0)) {
  $g.ReleaseHdc($hdc)
  $g.CopyFromScreen($r.left, $r.top, 0, 0, $bmp.Size)
} else {
  $g.ReleaseHdc($hdc)
}
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"saved $Out pw=$okpw"

