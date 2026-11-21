Add-Type -AssemblyName System.Drawing

# 多尺寸 ICO：16/32/48/64/128/256，各自按目标尺寸重画，小尺寸更锐利
function New-IcoBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)

    function RoundedRect($x, $y, $w, $h, $r) {
        $p = New-Object System.Drawing.Drawing2D.GraphicsPath
        $d = $r * 2
        if ($d -gt $w) { $d = $w }
        if ($d -gt $h) { $d = $h }
        $p.AddArc($x, $y, $d, $d, 180, 90)
        $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
        $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
        $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
        return $p
    }

    $k = $size / 256.0

    # 底：圆角方块
    $pad = [Math]::Max(1, [Math]::Round(8 * $k))
    $r = [Math]::Max(1, [Math]::Round(56 * $k))
    $path = RoundedRect $pad $pad ($size - $pad * 2) ($size - $pad * 2) $r
    $brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 47, 107, 240))
    $g.FillPath($brush, $path)

    # 纸页
    $pw = [Math]::Round(108 * $k)
    $ph = [Math]::Round(144 * $k)
    $px = [Math]::Round(($size - $pw) / 2)
    $py = [Math]::Round(($size - $ph) / 2)
    $page = RoundedRect $px $py $pw $ph ([Math]::Max(1, [Math]::Round(18 * $k)))
    $wb = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::White)
    $g.FillPath($wb, $page)

    # 三条文字线（小尺寸也保证是 1px 起）
    $line = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 47, 107, 240))
    $lw = [Math]::Max(1, [Math]::Round(12 * $k))
    $lx = [Math]::Round($px + 22 * $k)
    foreach ($i in 0..2) {
        $w2 = if ($i -lt 2) { [Math]::Round(64 * $k) } else { [Math]::Round(40 * $k) }
        $y2 = [Math]::Round($py + (40 + $i * 34) * $k)
        $g.FillRectangle($line, $lx, $y2, ([Math]::Max(2, $w2)), $lw)
    }

    $g.Dispose()
    return $bmp
}

$sizes = 256, 128, 64, 48, 32, 16
$entries = @()
foreach ($sz in $sizes) {
    $bmp = New-IcoBitmap $sz
    $ms = New-Object IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $entries += ,@($sz, $ms.ToArray())
}
# 256 单独留一份做预览
$big = New-IcoBitmap 256
$big.Save("$env:TEMP\scheda_icon.png", [System.Drawing.Imaging.ImageFormat]::Png)
$big.Dispose()

$count = $entries.Count
$ico = New-Object IO.MemoryStream
$bw = New-Object IO.BinaryWriter($ico)
$bw.Write([UInt16]0)          # reserved
$bw.Write([UInt16]1)          # type = icon
$bw.Write([UInt16]$count)     # count
$offset = 6 + 16 * $count
foreach ($e in $entries) {
    $sz = $e[0]
    $data = $e[1]
    $dimen = if ($sz -ge 256) { 0 } else { $sz }
    $bw.Write([Byte]$dimen)   # width
    $bw.Write([Byte]$dimen)   # height
    $bw.Write([Byte]0)        # palette
    $bw.Write([Byte]0)        # reserved
    $bw.Write([UInt16]1)      # planes
    $bw.Write([UInt16]32)     # bpp
    $bw.Write([UInt32]$data.Length)
    $bw.Write([UInt32]$offset)
    $offset += $data.Length
}
foreach ($e in $entries) { $bw.Write($e[1]) }
$bw.Flush()

$dist = "D:\20533\deskbox\fangwork\Scheda\dist\scheda.ico"
[IO.File]::WriteAllBytes($dist, $ico.ToArray())
New-Item -ItemType Directory -Force -Path "D:\20533\deskbox\fangwork\Scheda\assets" | Out-Null
Copy-Item $dist "D:\20533\deskbox\fangwork\Scheda\assets\scheda.ico" -Force
"ico {0:N0} bytes / {1} sizes" -f (Get-Item $dist).Length, $count
