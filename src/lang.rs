//! 界面文案：中文是源串，英文查表。`t("目录")` 在英文模式下回 "Contents"。
//! 只翻译看得见的 UI，内部日志/错误提示保持中文原样。

use std::sync::atomic::{AtomicU8, Ordering};

pub const ZH: u8 = 0;
pub const EN: u8 = 1;

static LANG: AtomicU8 = AtomicU8::new(ZH);

pub fn set(l: u8) {
    LANG.store(if l == EN { EN } else { ZH }, Ordering::Relaxed);
}

pub fn get() -> u8 {
    LANG.load(Ordering::Relaxed)
}

pub fn is_en() -> bool {
    get() == EN
}

/// 语言代码（写 settings.json 用）
pub fn code() -> &'static str {
    if is_en() { "en" } else { "zh" }
}

pub fn from_code(s: &str) -> u8 {
    if s.trim().eq_ignore_ascii_case("en") { EN } else { ZH }
}

/// 从 settings.json 读语言偏好
pub fn load() {
    if let Ok(txt) = std::fs::read_to_string(crate::theme::settings_path()) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
            if let Some(l) = v.get("lang").and_then(|x| x.as_str()) {
                set(from_code(l));
            }
        }
    }
}

pub fn t(zh: &'static str) -> &'static str {
    if !is_en() {
        return zh;
    }
    for (k, v) in TABLE {
        if *k == zh {
            return v;
        }
    }
    zh
}

static TABLE: &[(&str, &str)] = &[
    // —— 标题栏 / 通用
    ("主题", "Theme"),
    ("目录", "Contents"),
    ("设置", "Settings"),
    ("浅色", "Light"),
    ("深色", "Dark"),
    ("只读", "Read-only"),
    ("编辑", "Edit"),
    ("编辑中 · Ctrl+S 保存", "Editing · Ctrl+S to save"),
    ("已保存", "Saved"),
    ("已复制", "Copied"),
    ("复制", "Copy"),
    ("关闭", "Close"),
    ("语言", "Language"),
    ("中文", "Chinese"),
    ("English", "English"),
    // —— 欢迎页
    ("把 Markdown 读成它该有的样子", "Markdown, as it should look"),
    ("把 .md 文件拖进来", "Drop a .md file here"),
    ("或者按 Ctrl+O 选一个文件 — 双击就开，看完就关", "or press Ctrl+O — opens instantly, closes instantly"),
    ("最近打开", "Recent"),
    ("点一下就开", "Click to open"),
    ("还没有打开过文件 · 拖一个 .md 进来，或者按 Ctrl+O", "Nothing opened yet · drop a .md here, or press Ctrl+O"),
    ("小方工作室 · 零联网 · 零常驻", "Fang Studio · offline · zero footprint"),
    ("零常驻 · 零联网 · 冷启动几十毫秒。", "No resident process, no network, starts in milliseconds."),
    ("Markdown 查看器", "Markdown viewer"),
    // —— 设置
    ("外观与排版 · 改动即时生效，自动写进 settings.json", "Appearance · changes apply instantly and persist to settings.json"),
    ("选中即换 · 可切深浅", "Switch instantly · light or dark"),
    ("强调色", "Accent"),
    ("形态", "Shape"),
    ("圆角", "Radius"),
    ("磨砂", "Frost"),
    ("字号", "Text size"),
    ("行距", "Line height"),
    ("段距", "Paragraph gap"),
    ("阴影", "Shadow"),
    ("纸面", "Paper"),
    ("字体", "Fonts"),
    ("点一下换下一款", "Click to cycle"),
    ("界面", "UI"),
    ("正文", "Body"),
    ("代码", "Code"),
    ("背景", "Backdrop"),
    ("内置壁纸", "Built-in wallpaper"),
    ("自定义图片", "Custom image"),
    ("已关闭", "Off"),
    ("换图…", "Choose…"),
    ("预览", "Preview"),
    ("字体没装会自动回退到微软雅黑 / Segoe UI，照样能读。", "Missing fonts fall back to Segoe UI / Microsoft YaHei — still readable."),
    ("所有改动都写进 %APPDATA%\\Scheda\\settings.json", "Everything is written to %APPDATA%\\Scheda\\settings.json"),
    ("恢复当前预设", "Reset this preset"),
    ("打开主题文件夹", "Open themes folder"),
    ("设为默认 .md 打开方式", "Make default for .md"),
    ("背景已换", "Backdrop changed"),
    ("已注册 · 资源管理器里选「Scheda」设为默认", "Registered · pick Scheda in Explorer to make it default"),
    ("注册没写成，可能被系统拦了", "Registry write failed"),
    // —— 文档
    ("字 · 约 {} 分钟", "chars · about {} min"),
    ("零联网 · 远程图片已跳过", "Offline · remote image skipped"),
    ("零联网 · 未加载：{}", "Offline · not loaded: {}"),
    ("打不开这个文件", "Cannot open this file"),
    ("这个文件已经不在了", "This file is gone"),
    ("本地文件", "Local file"),
    ("打开 Markdown", "Open Markdown"),
    ("挑一张背景图", "Pick a backdrop"),
    ("图片", "Images"),
    ("所有文件", "All files"),
    // —— 图片查看器
    ("适应窗口", "Fit"),
    ("原始大小", "Actual size"),
    ("复制到剪贴板", "Copy image"),
    ("图片已复制", "Image copied"),
    ("滚轮缩放 · 拖动平移 · Ctrl+C 复制 · Esc 关闭", "Wheel to zoom · drag to pan · Ctrl+C to copy · Esc to close"),
    ("厚", "Thick"),
    ("后面那层被磨过", "blurs what's behind"),
    ("所有改动都写进 %APPDATA%\\Scheda\\settings.json", "Everything is saved to %APPDATA%\\Scheda\\settings.json"),
    ("打开", "Open"),
];

/// 动态文案两段式：中文前缀 + 数字 + 后缀。
pub fn meta_line(chars: usize, mins: u32) -> String {
    if is_en() {
        format!("{} chars · about {} min", chars, mins)
    } else {
        format!("{} 字 · 约 {} 分钟", chars, mins)
    }
}
