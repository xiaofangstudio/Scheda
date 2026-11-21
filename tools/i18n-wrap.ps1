$root = "D:\20533\deskbox\fangwork\Scheda"
$pairs = @(
  @("把 Markdown 读成它该有的样子", "Markdown, as it should look"),
  @("把 .md 文件拖进来", "Drop a .md file here"),
  @("或者按 Ctrl+O 选一个文件 — 双击就开，看完就关", "or press Ctrl+O — opens instantly, closes instantly"),
  @("最近打开", "Recent"),
  @("点一下就开", "Click to open"),
  @("还没有打开过文件 · 拖一个 .md 进来，或者按 Ctrl+O", "Nothing opened yet · drop a .md here, or press Ctrl+O"),
  @("小方工作室 · 零联网 · 零常驻", "Fang Studio · offline · zero footprint"),
  @("零常驻 · 零联网 · 冷启动几十毫秒。", "No resident process, no network, starts in milliseconds."),
  @("Markdown 查看器", "Markdown viewer"),
  @("外观与排版 · 改动即时生效，自动写进 settings.json", "Appearance · applies instantly, saved to settings.json"),
  @("选中即换 · 可切深浅", "Switch instantly · light or dark"),
  @("强调色", "Accent"),
  @("形态", "Shape"),
  @("圆角", "Radius"),
  @("磨砂", "Frost"),
  @("厚", "Thick"),
  @("后面那层被磨过", "blurs what's behind"),
  @("字号", "Text size"),
  @("行距", "Line height"),
  @("段距", "Paragraph gap"),
  @("阴影", "Shadow"),
  @("纸面", "Paper"),
  @("字体", "Fonts"),
  @("点一下换下一款", "Click to cycle"),
  @("界面", "UI"),
  @("正文", "Body"),
  @("代码", "Code"),
  @("背景", "Backdrop"),
  @("内置壁纸", "Built-in wallpaper"),
  @("自定义图片", "Custom image"),
  @("已关闭", "Off"),
  @("换图…", "Choose…"),
  @("预览", "Preview"),
  @("字体没装会自动回退到微软雅黑 / Segoe UI，照样能读。", "Missing fonts fall back to Segoe UI / Microsoft YaHei."),
  @("所有改动都写进 %APPDATA%\Scheda\settings.json", "Everything is saved to %APPDATA%\Scheda\settings.json"),
  @("恢复当前预设", "Reset this preset"),
  @("打开主题文件夹", "Open themes folder"),
  @("背景已换", "Backdrop changed"),
  @("已注册 · 资源管理器里选「Scheda」设为默认", "Registered · pick Scheda in Explorer to make it default"),
  @("注册没写成，可能被系统拦了", "Registry write failed"),
  @("打不开这个文件", "Cannot open this file"),
  @("这个文件已经不在了", "This file is gone"),
  @("本地文件", "Local file"),
  @("打开 Markdown", "Open Markdown"),
  @("打开", "Open"),
  @("挑一张背景图", "Pick a backdrop"),
  @("已复制", "Copied"),
  @("已保存", "Saved"),
  @("复制", "Copy"),
  @("关闭", "Close"),
  @("适应窗口", "Fit"),
  @("原始大小", "Actual size"),
  @("图片已复制", "Image copied"),
  @("滚轮缩放 · 拖动平移 · Ctrl+C 复制 · Esc 关闭", "Wheel to zoom · drag to pan · Ctrl+C to copy · Esc to close"),
  @("目录", "Contents"),
  @("设置", "Settings"),
  @("主题", "Theme"),
  @("浅色", "Light"),
  @("深色", "Dark"),
  @("语言", "Language"),
  @("中文", "Chinese"),
  @("设为默认 .md 打开方式", "Make default for .md")
)

foreach ($f in @("$root\src\main.rs", "$root\src\doc.rs")) {
  $txt = [System.IO.File]::ReadAllText($f)
  foreach ($p in $pairs) {
    $zh = $p[0]
    $esc = [Regex]::Escape($zh)
    $txt = [Regex]::Replace($txt, '(?<!t\()"' + $esc + '"', ('crate::lang::t("' + $zh + '")'))
  }
  [System.IO.File]::WriteAllText($f, $txt)
}

# lang.rs：补齐缺的词条
$lf = "$root\src\lang.rs"
$lt = [System.IO.File]::ReadAllText($lf)
$add = ""
foreach ($p in $pairs) {
  $zh = $p[0]; $en = $p[1]
  if ($lt -notmatch [Regex]::Escape('("' + $zh + '",')) {
    $add += '    ("' + $zh + '", "' + $en + '"),' + "`r`n"
  }
}
if ($add -ne "") {
  $idx = $lt.LastIndexOf("];")
  if ($idx -gt 0) {
    $lt = $lt.Substring(0, $idx) + $add + $lt.Substring($idx)
    [System.IO.File]::WriteAllText($lf, $lt)
  }
}
"done"
