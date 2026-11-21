Add-Type -AssemblyName System.Drawing

# .md 文档图标：白纸 + 折角 + 蓝色「MD」字，跟应用图标区分开
function New-MdBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
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

    # 纸页（白色，略微内缩，留出折角）
    $pw = [Math]::Round(150 * $k)
    $ph = [Math]::Round(190 * $k)
    $px = [Math]::Round((256 * $k - $pw) / 2)
    $py = [Math]::Round(34 * $k)
    $page = RoundedRect $px $py $pw $ph ([Math]::Max(1, [Math]::Round(14 * $k)))
    $wb = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::White)
    $g.FillPath($wb, $page)

    # 纸的描边（浅灰，透明底上才看得见白纸边）
    $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(90, 60, 70, 85)), ([Math]::Max(1, [Math]::Round(4 * $k)))
    $g.DrawPath($pen, $page)

    # 右上折角：一块蓝色三角
    $c = [Math]::Max(6, [Math]::Round(44 * $k))
    $tri = New-Object System.Drawing.Drawing2D.GraphicsPath
    $tri.AddPolygon([System.Drawing.PointF[]]@(
        (New-Object System.Drawing.PointF([float]($px + $pw - $c), [float]$py)),
        (New-Object System.Drawing.PointF([float]($px + $pw), [float]($py + $c))),
        (New-Object System.Drawing.PointF([float]($px + $pw - $c), [float]($py + $c))
    )))
    $blue = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 47, 107, 240))
    $g.FillPath($blue, $tri)

    # 「MD」字：小尺寸就直接画两条粗竖线，保证认得出来
    $bandY = [Math]::Round($py + $ph - 66 * $k)
    if ($size -ge 32) {
        $font = New-Object System.Drawing.Font("Segoe UI", ([float]([Math]::Max(9, 54 * $k))), [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
        $fmt = New-Object System.Drawing.StringFormat
        $fmt.Alignment = [System.Drawing.StringAlignment]::Center
        $fmt.LineAlignment = [System.Drawing.StringAlignment]::Center
        $rect = New-Object System.Drawing.RectangleF([float]$px, [float]$bandY, [float]$pw, [float]([Math]::Round(56 * $k)))
        $g.DrawString("MD", $font, $blue, $rect, $fmt)
        $font.Dispose()
    } else {
        $t = [Math]::Max(1, [Math]::Round(6 * $k))
        $g.FillRectangle($blue, [Math]::Round($px + 34 * $k), $bandY, $t, [Math]::Round(36 * $k))
        $g.FillRectangle($blue, [Math]::Round($px + 34 * $k), $bandY, [Math]::Round(20 * $k), $t)
        $g.FillRectangle($blue, [Math]::Round($px + 84 * $k), $bandY, $t, [Math]::Round(36 * $k))
    }

    $g.Dispose()
    return $bmp
}

$sizes = 256, 128, 64, 48, 32, 16
$entries = @()
foreach ($sz in $sizes) {
    $bmp = New-MdBitmap $sz
    $ms = New-Object IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    $entries += ,@($sz, $ms.ToArray())
}
$big = New-MdBitmap 256
$big.Save("$env:TEMP\md_icon.png", [System.Drawing.Imaging.ImageFormat]::Png)
$big.Dispose()

$count = $entries.Count
$ico = New-Object IO.MemoryStream
$bw = New-Object IO.BinaryWriter($ico)
$bw.Write([UInt16]0)
$bw.Write([UInt16]1)
$bw.Write([UInt16]$count)
$offset = 6 + 16 * $count
foreach ($e in $entries) {
    $sz = $e[0]
    $data = $e[1]
    $dimen = if ($sz -ge 256) { 0 } else { $sz }
    $bw.Write([Byte]$dimen)
    $bw.Write([Byte]$dimen)
    $bw.Write([Byte]0)
    $bw.Write([Byte]0)
    $bw.Write([UInt16]1)
    $bw.Write([UInt16]32)
    $bw.Write([UInt32]$data.Length)
    $bw.Write([UInt32]$offset)
    $offset += $data.Length
}
foreach ($e in $entries) { $bw.Write($e[1]) }
$bw.Flush()

$root = "D:\20533\deskbox\fangwork\Scheda"
[IO.File]::WriteAllBytes("$root\dist\md.ico", $ico.ToArray())
New-Item -ItemType Directory -Force -Path "$root\assets" | Out-Null
Copy-Item "$root\dist\md.ico" "$root\assets\md.ico" -Force
"md.ico {0:N0} bytes / {1} sizes" -f (Get-Item "$root\dist\md.ico").Length, $count
