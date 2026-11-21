Add-Type @"
using System; using System.Runtime.InteropServices;
public class HT {
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 public struct RECT { public int left,top,right,bottom; }
}
"@
$h = (Get-Process scheda | Select-Object -First 1).MainWindowHandle
$r = New-Object HT+RECT
[HT]::GetWindowRect($h, [ref]$r) | Out-Null
$L = $r.left; $T = $r.top; $R = $r.right; $B = $r.bottom
$pts = @(
  @(($L + 1), ($T + 1), 'tl'),
  @(($R - 1), ($T + 1), 'tr'),
  @(($L + 1), ($B - 1), 'bl'),
  @(($R - 1), ($B - 1), 'br'),
  @(($L + 1), ($T + 100), 'left'),
  @(($R - 1), ($T + 100), 'right'),
  @(($L + 100), ($T + 1), 'top'),
  @(($L + 100), ($B - 1), 'bottom'),
  @(($L + 300), ($T + 20), 'caption')
)
foreach ($p in $pts) {
  $lp = [IntPtr]::new(([int]$p[1] -shl 16) -bor [int]$p[0])
  $v = [HT]::SendMessage($h, 0x0084, $lp, [IntPtr]::Zero).ToInt64()
  "$($p[2]) => $v"
}
