param([string]$Out = "$env:TEMP\cap.png")
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Cap {
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
}
"@
Add-Type -AssemblyName System.Drawing
$p = Get-Process scheda -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { throw "scheda not running" }
$h = $p.MainWindowHandle
# 尺寸以 App 自己 paint 日志为准（同一会话里最后一条）
$trace = "$env:TEMP\scheda-shot-trace.txt"
$pw = 1680; $ph = 1170
if (Test-Path $trace) {
  $last = Get-Content $trace | Where-Object { $_ -match 'paint w=(\d+) h=(\d+)' } | Select-Object -Last 1
  if ($last -match 'paint w=(\d+) h=(\d+)') { $pw = [int]$Matches[1]; $ph = [int]$Matches[2] }
}
$bmp = New-Object System.Drawing.Bitmap $pw, $ph
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
[Cap]::PrintWindow($h, $hdc, 2) | Out-Null
$g.ReleaseHdc($hdc)
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
"saved $Out ${pw}x${ph}"
