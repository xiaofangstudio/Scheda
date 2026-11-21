$fence = [string]::new([char]96, 3)
$sb = New-Object System.Text.StringBuilder
for ($i = 0; $i -lt 1000; $i++) {
    [void]$sb.AppendLine("## 小节 $i")
    [void]$sb.AppendLine("")
    [void]$sb.AppendLine("这是第 $i 段，用来测试长文档的排版与滚动。文字重复几次凑长度，看看会不会卡。")
    [void]$sb.AppendLine("第二行继续说点什么，保持段落大约三行。")
    [void]$sb.AppendLine("")
    [void]$sb.AppendLine($fence + "rust")
    [void]$sb.AppendLine("fn f$i() -> u32 { $i }")
    [void]$sb.AppendLine($fence)
    [void]$sb.AppendLine("")
}
[IO.File]::WriteAllText("D:\20533\deskbox\fangwork\Scheda\test\big.md", $sb.ToString())
"lines: " + (Get-Content "D:\20533\deskbox\fangwork\Scheda\test\big.md").Count

$sw = [Diagnostics.Stopwatch]::StartNew()
$p = Start-Process -FilePath "D:\20533\deskbox\fangwork\Scheda\target\release\scheda.exe" -ArgumentList "D:\20533\deskbox\fangwork\Scheda\test\big.md" -PassThru
while ($p.MainWindowHandle -eq 0 -and $sw.ElapsedMilliseconds -lt 10000) { Start-Sleep -Milliseconds 5; $p.Refresh() }
"window ms: " + $sw.ElapsedMilliseconds
Start-Sleep -Milliseconds 1500
"ws MB: {0:N1}" -f ($p.WorkingSet64 / 1mb)
Stop-Process -Id $p.Id -Force
