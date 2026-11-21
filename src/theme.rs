//! 主题：内置预设 + 用户可编辑的 JSON 主题。
//!
//! 用户自定义主题放在 `%APPDATA%\Scheda\themes\*.json`，字段见 `Theme::apply_json`。
//! 同一份 JSON 也可以写进 settings.json 的 overrides，做到「改一行就变」。

use std::path::{Path, PathBuf};

pub type Color = [f32; 4];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Dark,
    Light,
}

pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a]
}

pub fn alpha(c: Color, a: f32) -> Color {
    [c[0], c[1], c[2], a]
}

/// 把「半透明色 + 已知纯色底」预先算成一个不透明色。
/// DC 渲染目标上的半透明大填充混合不稳，浅色界面统一用这个算实色。
pub fn over(c: Color, base: Color, k: f32) -> Color {
    let a = k.clamp(0.0, 1.0);
    [
        c[0] * a + base[0] * (1.0 - a),
        c[1] * a + base[1] * (1.0 - a),
        c[2] * a + base[2] * (1.0 - a),
        1.0,
    ]
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// 把 "#RRGGBB" / "#RRGGBBAA" / "#RGB" 解析成颜色。
pub fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    let hex = |c: u8| -> Option<f32> {
        match c {
            b'0'..=b'9' => Some((c - b'0') as f32),
            b'a'..=b'f' => Some((c - b'a' + 10) as f32),
            b'A'..=b'F' => Some((c - b'A' + 10) as f32),
            _ => None,
        }
    };
    let v = s.as_bytes();
    let mut out = [0f32; 4];
    out[3] = 1.0;
    match v.len() {
        3 => {
            for i in 0..3 {
                out[i] = (hex(v[i])? * 16.0 + hex(v[i])?) / 255.0;
            }
        }
        6 | 8 => {
            for i in 0..(v.len() / 2) {
                let hi = hex(v[i * 2])?;
                let lo = hex(v[i * 2 + 1])?;
                out[i] = (hi * 16.0 + lo) / 255.0;
            }
        }
        _ => return None,
    }
    Some(out)
}

pub fn to_hex(c: Color) -> String {
    let b = |f: f32| (f.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", b(c[0]), b(c[1]), b(c[2]))
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: String,
    pub mode: Mode,

    // —— 底色与光晕（磨砂玻璃背后透过来的东西）
    pub bg_top: Color,
    pub bg_bottom: Color,
    pub glow_a: Color,
    pub glow_b: Color,
    pub glow_a_pos: [f32; 2], // 0..1 归一化位置
    pub glow_b_pos: [f32; 2],
    pub glow_size: f32,

    // —— 玻璃
    pub accent: Color,
    pub accent2: Color,
    pub sheet_alpha: f32,   // 阅读纸面（磨砂板）
    pub panel_alpha: f32,   // 侧栏 / 面板
    pub border_alpha: f32,  // 发丝边
    pub frost: f32,         // 磨砂噪点强度 0..1
    pub radius: f32,        // 圆角（逻辑像素）
    pub shadow: f32,        // 投影强度

    // —— 字
    pub text: Color,
    pub muted: Color,
    pub heading: Color,
    pub link: Color,
    pub font_size: f32,
    pub line_height: f32,
    pub para_gap: f32,      // 段间距（逻辑像素）
    pub font_ui: String,
    pub font_text: String,
    pub font_mono: String,

    // —— 代码与块
    pub code_bg: Color,
    pub code_text: Color,
    pub tok_keyword: Color,
    pub tok_string: Color,
    pub tok_comment: Color,
    pub tok_number: Color,
    pub tok_type: Color,
    pub quote_bg: Color,
}

impl Theme {
    pub fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }

    /// 用 JSON 覆盖任意字段（用户自定义主题就是靠这个）。
    pub fn apply_json(&mut self, v: &serde_json::Value) {
        let get_color = |k: &str| -> Option<Color> { v.get(k)?.as_str().and_then(parse_hex) };
        let get_f = |k: &str| -> Option<f32> { v.get(k)?.as_f64().map(|x| x as f32) };
        let get_s = |k: &str| -> Option<String> { v.get(k)?.as_str().map(|s| s.to_string()) };

        if let Some(n) = get_s("name") {
            self.name = n;
        }
        if let Some(m) = get_s("mode") {
            self.mode = if m.eq_ignore_ascii_case("light") { Mode::Light } else { Mode::Dark };
        }
        if let Some(c) = get_color("bg") {
            self.bg_top = c;
            self.bg_bottom = mix(c, rgba(0, 0, 0, 1.0), 0.06);
        }
        if let Some(c) = get_color("bg_top") {
            self.bg_top = c;
        }
        if let Some(c) = get_color("bg_bottom") {
            self.bg_bottom = c;
        }
        if let Some(c) = get_color("glow_a") {
            self.glow_a = c;
        }
        if let Some(c) = get_color("glow_b") {
            self.glow_b = c;
        }
        if let Some(c) = get_color("accent") {
            self.accent = c;
            self.link = c;
        }
        if let Some(c) = get_color("accent2") {
            self.accent2 = c;
            self.tok_keyword = c;
        }
        if let Some(c) = get_color("text") {
            self.text = c;
        }
        if let Some(c) = get_color("muted") {
            self.muted = c;
            self.tok_comment = c;
        }
        if let Some(c) = get_color("heading") {
            self.heading = c;
        }
        if let Some(c) = get_color("link") {
            self.link = c;
        }
        if let Some(c) = get_color("code_bg") {
            self.code_bg = c;
            self.quote_bg = c;
        }
        if let Some(c) = get_color("code_text") {
            self.code_text = c;
        }
        for (k, slot) in [
            ("tok_keyword", &mut self.tok_keyword as *mut Color),
            ("tok_string", &mut self.tok_string as *mut Color),
            ("tok_comment", &mut self.tok_comment as *mut Color),
            ("tok_number", &mut self.tok_number as *mut Color),
            ("tok_type", &mut self.tok_type as *mut Color),
        ] {
            if let Some(c) = get_color(k) {
                unsafe { *slot = c };
            }
        }
        if let Some(x) = get_f("sheet_alpha") {
            self.sheet_alpha = x.clamp(0.80, 1.0);
        }
        if let Some(x) = get_f("panel_alpha") {
            self.panel_alpha = x;
        }
        if let Some(x) = get_f("border_alpha") {
            self.border_alpha = x;
        }
        if let Some(x) = get_f("frost") {
            self.frost = x.clamp(0.0, 1.0);
        }
        if let Some(x) = get_f("radius") {
            self.radius = x.clamp(0.0, 40.0);
        }
        if let Some(x) = get_f("shadow") {
            self.shadow = x.clamp(0.0, 1.0);
        }
        if let Some(x) = get_f("font_size") {
            self.font_size = x.clamp(11.0, 30.0);
        }
        if let Some(x) = get_f("line_height") {
            self.line_height = x.clamp(1.2, 2.6);
        }
        if let Some(x) = get_f("para_gap") {
            self.para_gap = x.clamp(2.0, 44.0);
        }
        if let Some(x) = get_s("font_ui") {
            self.font_ui = x;
        }
        if let Some(x) = get_s("font_text") {
            self.font_text = x;
        }
        if let Some(x) = get_s("font_mono") {
            self.font_mono = x;
        }
    }
}

// ————————————————————————————————————————————————————————————
// 内置主题
// ————————————————————————————————————————————————————————————

fn base() -> Theme {
    Theme {
        name: "极光".into(),
        mode: Mode::Light,
        bg_top: rgba(245, 249, 251, 1.0),
        bg_bottom: rgba(245, 249, 251, 1.0),
        glow_a: rgba(255, 255, 255, 0.0),
        glow_b: rgba(255, 255, 255, 0.0),
        glow_a_pos: [0.86, 0.10],
        glow_b_pos: [0.08, 0.86],
        glow_size: 1.0,
        accent: rgba(20, 140, 195, 1.0),
        accent2: rgba(104, 100, 245, 1.0),
        sheet_alpha: 0.94,
        panel_alpha: 0.92,
        border_alpha: 0.12,
        frost: 0.35,
        radius: 18.0,
        shadow: 0.22,
        text: rgba(25, 32, 40, 1.0),
        muted: rgba(110, 122, 133, 1.0),
        heading: rgba(12, 16, 22, 1.0),
        link: rgba(12, 125, 170, 1.0),
        font_size: 16.0,
        line_height: 1.75,
        para_gap: 14.0,
        font_ui: "Segoe UI Variable Text".into(),
        font_text: "Segoe UI Variable Text".into(),
        font_mono: "Cascadia Mono".into(),
        code_bg: rgba(237, 242, 247, 1.0),
        code_text: rgba(36, 46, 57, 1.0),
        tok_keyword: rgba(124, 76, 200, 1.0),
        tok_string: rgba(24, 122, 90, 1.0),
        tok_comment: rgba(128, 140, 150, 1.0),
        tok_number: rgba(180, 100, 25, 1.0),
        tok_type: rgba(20, 100, 165, 1.0),
        quote_bg: rgba(20, 40, 60, 0.045),
    }
}

pub fn presets() -> Vec<Theme> {
    let mut v = Vec::new();

    // 1. 极光
    v.push(base());

    // 2. 雾凇 —— 冷灰底 + 冰青，安静
    let mut t = base();
    t.name = "雾凇".into();
    t.bg_top = rgba(240, 245, 248, 1.0);
    t.bg_bottom = rgba(240, 245, 248, 1.0);
    t.accent = rgba(58, 122, 165, 1.0);
    t.accent2 = rgba(120, 148, 210, 1.0);
    t.text = rgba(28, 36, 43, 1.0);
    t.muted = rgba(112, 126, 136, 1.0);
    t.heading = rgba(14, 22, 29, 1.0);
    t.link = rgba(52, 116, 160, 1.0);
    t.code_bg = rgba(232, 239, 244, 1.0);
    t.code_text = rgba(38, 48, 58, 1.0);
    t.tok_keyword = rgba(88, 96, 190, 1.0);
    t.tok_type = rgba(34, 108, 158, 1.0);
    t.sheet_alpha = 0.96;
    t.frost = 0.42;
    v.push(t);

    // 3. 墨玉 —— 浅底，玉绿强调
    let mut t = base();
    t.name = "墨玉".into();
    t.bg_top = rgba(242, 247, 243, 1.0);
    t.bg_bottom = rgba(242, 247, 243, 1.0);
    t.accent = rgba(38, 130, 100, 1.0);
    t.accent2 = rgba(150, 148, 88, 1.0);
    t.text = rgba(24, 34, 29, 1.0);
    t.muted = rgba(104, 122, 113, 1.0);
    t.heading = rgba(12, 20, 16, 1.0);
    t.link = rgba(32, 118, 92, 1.0);
    t.code_bg = rgba(232, 240, 233, 1.0);
    t.code_text = rgba(32, 46, 38, 1.0);
    t.tok_keyword = rgba(38, 130, 100, 1.0);
    t.tok_string = rgba(136, 118, 42, 1.0);
    t.tok_number = rgba(176, 112, 45, 1.0);
    t.tok_type = rgba(52, 118, 128, 1.0);
    t.radius = 16.0;
    t.sheet_alpha = 0.95;
    v.push(t);

    // 4. 霓虹 —— 浅紫底 + 品红强调
    let mut t = base();
    t.name = "霓虹".into();
    t.bg_top = rgba(250, 246, 251, 1.0);
    t.bg_bottom = rgba(250, 246, 251, 1.0);
    t.accent = rgba(190, 58, 155, 1.0);
    t.accent2 = rgba(106, 88, 216, 1.0);
    t.text = rgba(36, 28, 38, 1.0);
    t.muted = rgba(124, 110, 128, 1.0);
    t.heading = rgba(22, 14, 24, 1.0);
    t.link = rgba(176, 50, 142, 1.0);
    t.code_bg = rgba(244, 238, 246, 1.0);
    t.code_text = rgba(48, 36, 52, 1.0);
    t.tok_keyword = rgba(160, 58, 176, 1.0);
    t.tok_string = rgba(44, 130, 118, 1.0);
    t.tok_type = rgba(70, 96, 190, 1.0);
    t.radius = 20.0;
    t.sheet_alpha = 0.95;
    t.frost = 0.30;
    v.push(t);

    // 5. 纸白 —— 中性浅底，纸面
    let mut t = base();
    t.name = "纸白".into();
    t.mode = Mode::Light;
    t.bg_top = rgba(247, 248, 250, 1.0);
    t.bg_bottom = rgba(247, 248, 250, 1.0);
    t.accent = rgba(14, 143, 184, 1.0);
    t.accent2 = rgba(88, 81, 214, 1.0);
    t.text = rgba(27, 33, 41, 1.0);
    t.muted = rgba(114, 124, 134, 1.0);
    t.heading = rgba(12, 16, 22, 1.0);
    t.link = rgba(12, 132, 172, 1.0);
    t.sheet_alpha = 0.97;
    t.panel_alpha = 0.95;
    t.border_alpha = 0.10;
    t.code_bg = rgba(238, 243, 248, 1.0);
    t.code_text = rgba(40, 50, 60, 1.0);
    t.quote_bg = rgba(15, 30, 45, 0.05);
    t.tok_keyword = rgba(110, 60, 200, 1.0);
    t.tok_string = rgba(20, 120, 90, 1.0);
    t.tok_comment = rgba(130, 140, 150, 1.0);
    t.tok_number = rgba(185, 100, 20, 1.0);
    t.tok_type = rgba(20, 100, 160, 1.0);
    t.shadow = 0.20;
    t.frost = 0.28;
    v.push(t);

    // 6. 晨雾 —— 浅暖调
    let mut t = base();
    t.name = "晨雾".into();
    t.mode = Mode::Light;
    t.bg_top = rgba(250, 247, 241, 1.0);
    t.bg_bottom = rgba(250, 247, 241, 1.0);
    t.accent = rgba(190, 116, 56, 1.0);
    t.accent2 = rgba(108, 138, 106, 1.0);
    t.text = rgba(38, 32, 27, 1.0);
    t.muted = rgba(128, 118, 105, 1.0);
    t.heading = rgba(22, 18, 14, 1.0);
    t.link = rgba(172, 98, 44, 1.0);
    t.sheet_alpha = 0.97;
    t.panel_alpha = 0.95;
    t.code_bg = rgba(244, 239, 232, 1.0);
    t.code_text = rgba(52, 44, 36, 1.0);
    t.quote_bg = rgba(120, 80, 40, 0.05);
    t.tok_keyword = rgba(150, 70, 40, 1.0);
    t.tok_string = rgba(60, 120, 70, 1.0);
    t.tok_comment = rgba(140, 130, 118, 1.0);
    t.tok_number = rgba(160, 90, 40, 1.0);
    t.tok_type = rgba(50, 100, 140, 1.0);
    t.shadow = 0.18;
    v.push(t);

    v
}

// ————————————————————————————————————————————————————————————
// 设置 / 自定义主题目录
// ————————————————————————————————————————————————————————————

pub fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    Path::new(&base).join("Scheda")
}

pub fn themes_dir() -> PathBuf {
    data_dir().join("themes")
}

pub fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

fn read_settings() -> serde_json::Value {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn write_settings(v: &serde_json::Value) {
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(settings_path(), serde_json::to_string_pretty(v).unwrap_or_default());
}

/// 欢迎页的「最近打开」：从 settings.json 里读，最多 8 条，只留还在的文件。
pub fn load_recent() -> Vec<String> {
    let v = read_settings();
    let mut list: Vec<String> = v
        .get("recent")
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    list.retain(|p| Path::new(p).exists());
    list.truncate(8);
    list
}

/// 打开一个文件后把它顶到最前面（去重、截断、顺手写盘）。
pub fn add_recent(path: &Path) -> Vec<String> {
    let p = path.to_string_lossy().to_string();
    let mut list = load_recent();
    list.retain(|x| x != &p);
    list.insert(0, p);
    list.retain(|x| Path::new(x).exists());
    list.truncate(8);
    let mut v = read_settings();
    if let Some(o) = v.as_object_mut() {
        o.insert("recent".into(), serde_json::json!(list));
    }
    write_settings(&v);
    list
}

/// 首次运行时铺一份带注释意义的示例主题，让「能改」有东西可改。
pub fn ensure_theme_skeleton() {
    let dir = themes_dir();
    let _ = std::fs::create_dir_all(&dir);
    let sample = dir.join("我的主题.json");
    if !sample.exists() {
        let json = r##"{
  "name": "我的主题",
  "mode": "light",
  "bg": "#F7F8FA",
  "accent": "#0E8FB8",
  "accent2": "#5851D6",
  "text": "#1B2129",
  "muted": "#727C86",
  "heading": "#0C1016",
  "link": "#0C84AC",
  "sheet_alpha": 0.97,
  "panel_alpha": 0.95,
  "border_alpha": 0.10,
  "frost": 0.28,
  "radius": 18,
  "shadow": 0.20,
  "font_size": 16,
  "line_height": 1.75,
  "font_text": "Segoe UI Variable Text",
  "font_ui": "Segoe UI Variable Text",
  "font_mono": "Cascadia Mono",
  "code_bg": "#EEF3F8",
  "code_text": "#28323C",
  "tok_keyword": "#6E3CC8",
  "tok_string": "#14785A",
  "tok_comment": "#828C96",
  "tok_number": "#B96414",
  "tok_type": "#1464A0"
}
"##;
        let _ = std::fs::write(&sample, json);
    }
}

pub struct ThemeSet {
    pub all: Vec<Theme>,
    pub active: usize,
}

impl ThemeSet {
    pub fn load() -> ThemeSet {
        ensure_theme_skeleton();
        let mut all = presets();

        // 用户主题：themes/*.json
        if let Ok(rd) = std::fs::read_dir(themes_dir()) {
            let mut extra: Vec<Theme> = Vec::new();
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("json")).unwrap_or(false) {
                    if let Ok(txt) = std::fs::read_to_string(&p) {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                            let mut t = presets()[0].clone();
                            t.apply_json(&v);
                            if let Some(n) = v.get("base").and_then(|b| b.as_u64()) {
                                if let Some(pb) = presets().get(n as usize) {
                                    t = pb.clone();
                                    t.apply_json(&v);
                                }
                            }
                            if t.name.trim().is_empty() {
                                t.name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
                            }
                            extra.push(t);
                        }
                    }
                }
            }
            extra.sort_by(|a, b| a.name.cmp(&b.name));
            all.extend(extra);
        }

        // settings.json：记住选中的主题 + 滑杆覆盖值
        let mut active = 0usize;
        let mut overrides: Option<serde_json::Value> = None;
        if let Ok(txt) = std::fs::read_to_string(settings_path()) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if let Some(name) = v.get("theme").and_then(|x| x.as_str()) {
                    if let Some(i) = all.iter().position(|t| t.name == name) {
                        active = i;
                    }
                }
                overrides = v.get("overrides").cloned();
            }
        }
        if let Some(o) = overrides {
            all[active].apply_json(&o);
        }
        ThemeSet { all, active }
    }

    pub fn cur(&self) -> &Theme {
        &self.all[self.active]
    }

    pub fn save(&self, overrides: &serde_json::Value) {
        // 保留文件里其它键（比如 recent），只覆盖主题相关的两项
        let mut v = read_settings();
        if let Some(o) = v.as_object_mut() {
            o.insert("theme".into(), serde_json::json!(self.cur().name));
            if overrides.is_null() {
                o.remove("overrides");
            } else {
                o.insert("overrides".into(), overrides.clone());
            }
        }
        write_settings(&v);
    }
}
