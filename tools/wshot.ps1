param(
  [string]$Out = "$env:TEMP\wshot.png",
  [int]$W = 1590,
  [int]$H = 1110,
  [int]$Wheel = 0,
  [int]$CX = 1150,
  [int]$CY = 420
)
Get-Process scheda -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
$env:SCHEDA_TRACE = '1'
$trace = "$env:TEMP\scheda-shot-trace.txt"
Remove-Item $trace -ErrorAction SilentlyContinue
$p = Start-Process -FilePath "D:\20533\deskbox\fangwork\Scheda\target\release\scheda.exe" -ArgumentList "$env:TEMP\__no_such_file.md" -PassThru -RedirectStandardError $trace
Start-Sleep -Milliseconds 2400
Add-Type @"
using System; using System.Runtime.InteropServices;
public class WShot {
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x,int y,int w,int h2,uint f);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr wp, IntPtr lp);
 public struct POINT { public int x, y; }
}
"@
$h = $p.MainWindowHandle
[WShot]::SetWindowPos($h, [IntPtr](-2), 80, 40, $W, $H, 0x0040) | Out-Null
Start-Sleep -Milliseconds 400
[WShot]::SetWindowPos($h, [IntPtr](-2), 80, 40, $W, $H, 0x0040) | Out-Null
[WShot]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 900
if ($Wheel -ne 0) {
  $pt = New-Object WShot+POINT
  $pt.x = $CX; $pt.y = $CY
  [WShot]::ClientToScreen($h, [ref]$pt) | Out-Null
  $lp = [IntPtr]::new((($pt.y -band 0xFFFF) -shl 16) -bor ($pt.x -band 0xFFFF))
  for ($i = 0; $i -lt 6; $i++) {
    [WShot]::PostMessage($h, 0x020A, [IntPtr]::new(($Wheel -shl 16) -bor 0), $lp) | Out-Null
    Start-Sleep -Milliseconds 60
  }
  Start-Sleep -Milliseconds 900
}
$last = Get-Content $trace | Where-Object { $_ -match 'paint w=(\d+) h=(\d+)' } | Select-Object -Last 1
$pw = 0; $ph = 0
if ($last -match 'paint w=(\d+) h=(\d+)') { $pw = [int]$Matches[1]; $ph = [int]$Matches[2] }
Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap $pw, $ph
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
[WShot]::PrintWindow($h, $hdc, 2) | Out-Null
$g.ReleaseHdc($hdc)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose(); $g.Dispose()
"saved $Out $pw x $ph"
