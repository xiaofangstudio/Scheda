#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case, non_upper_case_globals)]

mod doc;
mod md;
mod theme;

use std::ffi::c_void;
use std::path::PathBuf;

use doc::Doc;
use md::Block;

use theme::{alpha, mix, Color, Theme, ThemeSet};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::ProcessStatus::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Controls::Dialogs::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::{Matrix3x2, Vector2};

const TITLE_H: f32 = 54.0;
const WM_MOUSE_LEAVE: u32 = 0x02A3;

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let instance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => return,
        };
        let class = w!("SchedaWnd");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let style = WS_POPUP | WS_THICKFRAME | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
        let hwnd = match CreateWindowExW(
            WS_EX_APPWINDOW,
            class,
            w!("Scheda"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1120,
            780,
            None,
            None,
            None,
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        // Windows 11 原生圆角 + 去掉系统描边（我们自己画发丝边）
        let corner = DWM_WINDOW_CORNER_PREFERENCE(2); // DWMWCP_ROUND
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(33),
            &corner as *const _ as *const c_void,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
        let border_none: u32 = 0xFFFF_FFFE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWINDOWATTRIBUTE(34), &border_none as *const _ as *const c_void, 4);

        let mut app = Box::new(App::new(hwnd));
        app.set_theme_dark_mode();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);

        center_window(hwnd);
        apply_icon(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetTimer(Some(hwnd), 2, 1500, None);
        DragAcceptFiles(hwnd, true);

        // 命令行参数：scheda.exe <file.md>
        if let Some(arg) = std::env::args().nth(1) {
            let p = PathBuf::from(arg);
            if p.exists() {
                if let Some(a) = app_mut(hwnd) {
                    a.open_file(&p);
                }
            }
        }

        loop {
            let mut msg = MSG::default();
            let r = GetMessageW(&mut msg, None, 0, 0);
            if r.0 <= 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe fn app_mut(hwnd: HWND) -> Option<&'static mut App> {
    let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if p.is_null() {
        None
    } else {
        Some(&mut *p)
    }
}

unsafe fn apply_icon(hwnd: HWND) {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join("scheda.ico"));
            cands.push(dir.join("assets").join("scheda.ico"));
            if let Some(up) = dir.parent() {
                cands.push(up.join("scheda.ico"));
                cands.push(up.join("assets").join("scheda.ico"));
                if let Some(up2) = up.parent() {
                    cands.push(up2.join("assets").join("scheda.ico"));
                }
            }
        }
    }
    for p in cands {
        if p.exists() {
            let s = HSTRING::from(p.to_string_lossy().to_string());
            // 小图标按系统尺寸取，大图标要 256 原图，任务栏 / Alt-Tab 才不糊
            let sm = GetSystemMetrics(SM_CXSMICON);
            if let Ok(hs) = LoadImageW(None, &s, IMAGE_ICON, sm, sm, LR_LOADFROMFILE) {
                let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(0)), Some(LPARAM(hs.0 as isize)));
            }
            if let Ok(hb) = LoadImageW(None, &s, IMAGE_ICON, 0, 0, LR_LOADFROMFILE) {
                let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(1)), Some(LPARAM(hb.0 as isize)));
            }
            break;
        }
    }
}

fn center_window(hwnd: HWND) {
    unsafe {
        // 高 DPI 上按比例放大默认尺寸，夹在工作区里
        let scale = {
            let d = GetDpiForWindow(hwnd);
            if d == 0 { 1.0 } else { d as f32 / 96.0 }
        };
        if scale > 1.05 {
            let mut want_w = (1100.0 * scale) as i32;
            let mut want_h = (760.0 * scale) as i32;
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
            if !mon.is_invalid() {
                let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                if GetMonitorInfoW(mon, &mut mi).as_bool() {
                    let ww = mi.rcWork.right - mi.rcWork.left;
                    let wh = mi.rcWork.bottom - mi.rcWork.top;
                    want_w = want_w.min((ww as f32 * 0.86) as i32);
                    want_h = want_h.min((wh as f32 * 0.90) as i32);
                }
            }
            let _ = SetWindowPos(hwnd, None, 0, 0, want_w, want_h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_err() {
            return;
        }
        let w = r.right - r.left;
        let h = r.bottom - r.top;
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
        if mon.is_invalid() {
            return;
        }
        {
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(mon, &mut mi).as_bool() {
                let x = mi.rcWork.left + (mi.rcWork.right - mi.rcWork.left - w) / 2;
                let y = mi.rcWork.top + (mi.rcWork.bottom - mi.rcWork.top - h) / 3;
                let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
            }
        }
    }
}

// ————————————————————————————————————————————————————————————————
// 图形设备（Direct2D / DirectWrite）
// ————————————————————————————————————————————————————————————————

struct Gfx {
    factory: ID2D1Factory1,
    dwrite: IDWriteFactory,
    rt: Option<ID2D1DCRenderTarget>,
    solid: Option<ID2D1SolidColorBrush>,
    noise: Option<ID2D1Bitmap>,
    noise_brush: Option<ID2D1BitmapBrush>,
    fmts: std::collections::HashMap<(String, u32, u32, bool), IDWriteTextFormat>,
    /// 正文排版用的格式（带行距），键里含行距与基线
    doc_fmts: std::collections::HashMap<(String, u32, u32, bool, u32, u32), IDWriteTextFormat>,
    ascents: std::collections::HashMap<(String, u32, u32), f32>,
    tbrushes: std::collections::HashMap<u32, IUnknown>,
    bg_key: (usize, i32, i32),
    bg: Option<BgBrushes>,
}

struct BgBrushes {
    base: ID2D1LinearGradientBrush,
    glow_a: ID2D1RadialGradientBrush,
    glow_b: ID2D1RadialGradientBrush,
}

fn fweight(w: i32) -> DWRITE_FONT_WEIGHT {
    match w {
        700 => DWRITE_FONT_WEIGHT_BOLD,
        600 => DWRITE_FONT_WEIGHT_SEMI_BOLD,
        500 => DWRITE_FONT_WEIGHT_MEDIUM,
        300 => DWRITE_FONT_WEIGHT_LIGHT,
        _ => DWRITE_FONT_WEIGHT_NORMAL,
    }
}

fn cf(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c[0], g: c[1], b: c[2], a: c[3] }
}

fn pt(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn rectf(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }
}

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: rectf(x, y, w, h), radiusX: r, radiusY: r }
}

fn ident() -> Matrix3x2 {
    Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 }
}

impl Gfx {
    unsafe fn new() -> Option<Gfx> {
        let factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;

        Some(Gfx {
            factory,
            dwrite,
            rt: None,
            solid: None,
            noise: None,
            noise_brush: None,
            fmts: std::collections::HashMap::new(),
            doc_fmts: std::collections::HashMap::new(),
            ascents: std::collections::HashMap::new(),
            tbrushes: std::collections::HashMap::new(),
            bg_key: (0, 0, 0),
            bg: None,
        })
    }

    /// DC 渲染目标：每帧从 WM_PAINT 的 HDC 绑定，稳定、零依赖。
    unsafe fn target(&mut self) -> Option<ID2D1DCRenderTarget> {
        if self.rt.is_none() {
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                // 统一按物理像素排版，DPI 固定成 96。
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            match self.factory.CreateDCRenderTarget(&props) {
                Ok(rt) => {
                    let _ = rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    let _ = rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
                    self.rt = Some(rt);
                    self.solid = None;
                    self.noise = None;
                    self.noise_brush = None;
                    self.fmts.clear();
                    self.doc_fmts.clear();
                    self.ascents.clear();
                    self.tbrushes.clear();
                    self.bg = None;
                    self.bg_key = (0, 0, 0);
                }
                Err(_) => return None,
            }
        }
        let rt = self.rt.as_ref()?.clone();

        if self.solid.is_none() {
            self.solid = rt.CreateSolidColorBrush(&cf([1.0, 1.0, 1.0, 1.0]), None).ok();
        }
        if self.noise.is_none() {
            self.noise = make_noise(&rt).ok();
            if let Some(n) = self.noise.as_ref() {
                let bprops = D2D1_BITMAP_BRUSH_PROPERTIES {
                    extendModeX: D2D1_EXTEND_MODE_WRAP,
                    extendModeY: D2D1_EXTEND_MODE_WRAP,
                    interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                };
                let props = D2D1_BITMAP_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                };
                let bp = D2D1_BRUSH_PROPERTIES { opacity: 1.0, transform: ident() };
                self.noise_brush = rt.CreateBitmapBrush(n, Some(&bprops), Some(&bp)).ok();
            }
        }
        Some(rt)
    }

    unsafe fn resize(&mut self, _w: i32, _h: i32) {
        self.bg = None;
        self.bg_key = (0, 0, 0);
    }

    unsafe fn solid(&mut self, rt: &ID2D1RenderTarget, c: Color) -> Option<&ID2D1SolidColorBrush> {
        if self.solid.is_none() {
            self.solid = rt.CreateSolidColorBrush(&cf([1.0, 1.0, 1.0, 1.0]), None).ok();
        }
        let b = self.solid.as_ref()?;
        let _ = b.SetColor(&cf(c));
        Some(b)
    }

    unsafe fn format(&mut self, family: &str, size: f32, weight: i32, italic: bool) -> Option<IDWriteTextFormat> {
        let key = (family.to_string(), (size * 10.0) as u32, weight as u32, italic);
        if !self.fmts.contains_key(&key) {
            let wstr = HSTRING::from(family);
            let locale = HSTRING::from("zh-cn");
            let weight: DWRITE_FONT_WEIGHT = fweight(weight);
            let style = if italic { DWRITE_FONT_STYLE_ITALIC } else { DWRITE_FONT_STYLE_NORMAL };
            match self.dwrite.CreateTextFormat(&wstr, None, weight, style, DWRITE_FONT_STRETCH_NORMAL, size, &locale) {
                Ok(f) => {
                    self.fmts.insert(key.clone(), f);
                }
                Err(e) => {
                    if std::env::var_os("SCHEDA_TRACE").is_some() {
                        eprintln!("CreateTextFormat failed for {family} size {size}: {e:?}");
                    }
                }
            }
        }
        self.fmts.get(&key).cloned()
    }

    /// 真实基线：用一次临时排版量出来，保证 UNIFORM 行距下文字不偏上/偏下
    unsafe fn ascent(&mut self, family: &str, size: f32, weight: i32) -> f32 {
        let key = (family.to_string(), (size * 100.0) as u32, weight as u32);
        if let Some(a) = self.ascents.get(&key) {
            return *a;
        }
        let mut a = size * 0.8;
        if let Some(fmt) = self.format(family, size, weight, false) {
            let u = text_w("Ag");
            if let Ok(l) = self.dwrite.CreateTextLayout(&u, &fmt, 600.0, 200.0) {
                let mut m = [DWRITE_LINE_METRICS::default(); 1];
                let mut n = 0u32;
                if l.GetLineMetrics(Some(&mut m), &mut n).is_ok() && n > 0 {
                    a = m[0].baseline;
                }
            }
        }
        self.ascents.insert(key, a);
        a
    }

    /// 按颜色缓存的实心画刷，用来给文字区段上色（DirectWrite 的 drawing effect）
    unsafe fn tbrush(&mut self, rt: &ID2D1RenderTarget, c: Color) -> Option<IUnknown> {
        let key = ((c[0].clamp(0.0, 1.0) * 255.0) as u32)
            | (((c[1].clamp(0.0, 1.0) * 255.0) as u32) << 8)
            | (((c[2].clamp(0.0, 1.0) * 255.0) as u32) << 16)
            | (((c[3].clamp(0.0, 1.0) * 255.0) as u32) << 24);
        if let Some(b) = self.tbrushes.get(&key) {
            return Some(b.clone());
        }
        let b = rt.CreateSolidColorBrush(&cf(c), None).ok()?;
        let u: IUnknown = b.cast().ok()?;
        self.tbrushes.insert(key, u.clone());
        Some(u)
    }

    /// 文档排版专用格式：带 UNIFORM 行距（正文可以按主题调行高）
    unsafe fn doc_format(&mut self, family: &str, size: f32, weight: i32, italic: bool, lh: f32) -> Option<IDWriteTextFormat> {
        let asc = self.ascent(family, size, weight);
        let key = (family.to_string(), (size * 10.0) as u32, weight as u32, italic, (lh * 100.0) as u32, (asc * 10.0) as u32);
        if !self.doc_fmts.contains_key(&key) {
            let f = self.format(family, size, weight, italic)?;
            let _ = f.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, size * lh, asc);
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            let _ = f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
            self.doc_fmts.insert(key.clone(), f);
        }
        self.doc_fmts.get(&key).cloned()
    }
}

unsafe fn make_noise(rt: &ID2D1RenderTarget) -> Result<ID2D1Bitmap> {
    const N: usize = 256;
    let mut px = vec![0u8; N * N * 4];
    let mut seed: u32 = 0x9e37_79b9;
    for i in 0..(N * N) {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let a = (seed >> 8) & 0xFF;
        let a = (a as f32 / 255.0).powf(1.6);
        let v = (a * 255.0) as u8;
        px[i * 4] = 255;
        px[i * 4 + 1] = 255;
        px[i * 4 + 2] = 255;
        px[i * 4 + 3] = v.min(140);
    }
    let props = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
    };
    rt.CreateBitmap(
        D2D_SIZE_U { width: N as u32, height: N as u32 },
        Some(px.as_ptr() as *const c_void),
        (N * 4) as u32,
        &props,
    )
}

fn text_w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

// ————————————————————————————————————————————————————————————————
// 命中目标
// ————————————————————————————————————————————————————————————————

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Hot {
    #[default]
    None,
    Min,
    Max,
    Close,
    Theme,
    Toc,
    Content,
    CopyCode(usize),
    TocItem(usize),
    Preset(usize),
    Swatch(usize),
    Slider(u8),
    Font(u8),
    PanelClose,
    OpenThemes,
    ResetTheme,
    Recent(usize),
}

#[derive(Clone, Copy, Debug, Default)]
struct R {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl R {
    fn hit(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
    fn cx(&self) -> f32 {
        self.x + self.w * 0.5
    }
    fn cy(&self) -> f32 {
        self.y + self.h * 0.5
    }
}

// ————————————————————————————————————————————————————————————————
// 应用状态
// ————————————————————————————————————————————————————————————————

struct App {
    hwnd: HWND,
    gfx: Gfx,
    themes: ThemeSet,
    scale: f32,
    w: i32,
    h: i32,

    hot: Hot,
    pressed: Hot,
    mouse: (f32, f32),

    scroll: f32,
    scroll_t: f32,

    toc_open: bool,
    settings: bool,
    dragging_slider: u8,

    theme_rev: u64,
    overrides: serde_json::Value,

    file: Option<(PathBuf, String)>,
    blocks: Option<Vec<Block>>,
    zoom: f32,
    doc: Option<Doc>,
    toc_entries: Vec<R>,

    // 窗口按钮 / 面板命中区
    btn_min: R,
    btn_max: R,
    btn_close: R,
    btn_theme: R,
    btn_toc: R,
    panel_rect: R,
    panel_x: R,
    chip_open: R,
    chip_themes: R,
    chip_reset: R,
    preset_rects: Vec<R>,
    swatch_rects: Vec<R>,
    slider_rects: [R; 7],
    font_rects: [R; 3],
    toc_rect: R,
    recent: Vec<String>,
    recent_rects: Vec<R>,

    status: Option<(String, f32)>,
}

impl App {
    unsafe fn new(hwnd: HWND) -> App {
        let gfx = Gfx::new().expect("Direct2D 初始化失败");
        let scale = {
            let d = GetDpiForWindow(hwnd);
            if d == 0 { 1.0 } else { d as f32 / 96.0 }
        };
        let mut themes = ThemeSet::load();
        themes.active = themes.active.min(themes.all.len().saturating_sub(1));
        // 只做浅色：万一旧设置里选的是深色主题，直接回到纸白
        if themes.all.get(themes.active).map(|t| t.is_dark()).unwrap_or(true) {
            themes.active = themes
                .all
                .iter()
                .position(|t| t.name == "纸白")
                .unwrap_or(0);
        }

        // 读取用户对当前主题的微调
        let mut overrides = serde_json::Value::Null;
        if let Ok(txt) = std::fs::read_to_string(theme::settings_path()) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) {
                if let Some(o) = v.get("overrides") {
                    overrides = o.clone();
                }
            }
        }

        let mut a = App {
            hwnd,
            gfx,
            themes,
            scale,
            w: 1,
            h: 1,
            hot: Hot::None,
            pressed: Hot::None,
            mouse: (0.0, 0.0),
            scroll: 0.0,
            scroll_t: 0.0,
            toc_open: true,
            settings: false,
            dragging_slider: 0,
            theme_rev: 0,
            overrides,
            file: None,
            blocks: None,
            zoom: 1.0,
            doc: None,
            toc_entries: Vec::new(),
            btn_min: R::default(),
            btn_max: R::default(),
            btn_close: R::default(),
            btn_theme: R::default(),
            btn_toc: R::default(),
            panel_rect: R::default(),
            panel_x: R::default(),
            chip_open: R::default(),
            chip_themes: R::default(),
            chip_reset: R::default(),
            preset_rects: Vec::new(),
            swatch_rects: Vec::new(),
            slider_rects: [R::default(); 7],
            font_rects: [R::default(); 3],
            toc_rect: R::default(),
            recent: theme::load_recent(),
            recent_rects: Vec::new(),
            status: None,
        };
        let mut r = RECT::default();
        if GetClientRect(hwnd, &mut r).is_ok() {
            a.w = r.right - r.left;
            a.h = r.bottom - r.top;
        }
        a
    }

    fn th(&self) -> &Theme {
        &self.themes.all[self.themes.active]
    }

    unsafe fn set_theme_dark_mode(&self) {
        let dark: BOOL = if self.th().is_dark() { BOOL(1) } else { BOOL(0) };
        let _ = DwmSetWindowAttribute(
            self.hwnd,
            DWMWINDOWATTRIBUTE(20),
            &dark as *const _ as *const c_void,
            std::mem::size_of::<BOOL>() as u32,
        );
    }

    unsafe fn open_file(&mut self, p: &PathBuf) {
        if let Ok(txt) = std::fs::read_to_string(p) {
            self.blocks = Some(md::parse(&txt));
            self.file = Some((p.clone(), txt));
            self.doc = None;
            self.scroll = 0.0;
            self.scroll_t = 0.0;
            self.recent = theme::add_recent(p);
            let name = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            let _ = SetWindowTextW(self.hwnd, &HSTRING::from(format!("{} — Scheda", name)));
            InvalidateRect(Some(self.hwnd), None, false);
        } else {
            self.status = Some(("打不开这个文件".to_string(), 2.0));
            InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn title(&self) -> String {
        match &self.file {
            Some((p, _)) => p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
            None => "Scheda".into(),
        }
    }

    fn subtitle(&self) -> String {
        match &self.file {
            Some((p, _)) => p.parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
            None => "Markdown 查看器".into(),
        }
    }

    // 计算窗口顶部按钮的命中区
    unsafe fn chrome(&mut self) {
        let s = self.scale;
        let th = TITLE_H * s;
        let bw = 34.0 * s;
        let bh = 28.0 * s;
        let cy = (th - bh) * 0.5;
        let right = self.w as f32 - 10.0 * s;
        self.btn_close = R { x: right - bw, y: cy, w: bw, h: bh };
        self.btn_max = R { x: right - bw * 2.0 - 2.0 * s, y: cy, w: bw, h: bh };
        self.btn_min = R { x: right - bw * 3.0 - 4.0 * s, y: cy, w: bw, h: bh };
        let after = self.btn_min.x - 8.0 * s;
        let tw = 74.0 * s;
        self.btn_theme = R { x: after - tw, y: cy, w: tw, h: bh };
        // 目录按钮：窗口够宽才出现（否则目录面板本身也放不下）
        let toc_ok = (self.w as f32) >= 1060.0 * s && self.file.is_some();
        self.btn_toc = if toc_ok { R { x: self.btn_theme.x - 44.0 * s, y: cy, w: 36.0 * s, h: bh } } else { R::default() };
        if self.btn_theme.x < 210.0 * s {
            self.btn_toc = R::default();
            self.btn_theme = R::default();
        }
    }

    unsafe fn hit(&self, x: f32, y: f32) -> Hot {
        let s = self.scale;
        // 设置页：整页命中区
        if self.settings && self.panel_rect.hit(x, y) {
            if let Some(i) = self.preset_rects.iter().position(|r| r.hit(x, y)) {
                return Hot::Preset(i);
            }
            if let Some(i) = self.swatch_rects.iter().position(|r| r.hit(x, y)) {
                return Hot::Swatch(i);
            }
            for (i, r) in self.slider_rects.iter().enumerate() {
                if r.hit(x, y) {
                    return Hot::Slider(i as u8);
                }
            }
            for (i, r) in self.font_rects.iter().enumerate() {
                if r.w > 1.0 && r.hit(x, y) {
                    return Hot::Font(i as u8);
                }
            }
            if self.chip_open.hit(x, y) {
                return Hot::OpenThemes;
            }
            if self.chip_reset.hit(x, y) {
                return Hot::ResetTheme;
            }
            if self.panel_x.hit(x, y) {
                return Hot::PanelClose;
            }
            return Hot::Content;
        }
        if self.btn_min.hit(x, y) {
            return Hot::Min;
        }
        if self.btn_max.hit(x, y) {
            return Hot::Max;
        }
        if self.btn_close.hit(x, y) {
            return Hot::Close;
        }
        if self.btn_theme.hit(x, y) {
            return Hot::Theme;
        }
        if self.btn_toc.hit(x, y) {
            return Hot::Toc;
        }
        for (i, r) in self.toc_entries.iter().enumerate() {
            if r.hit(x, y) {
                return Hot::TocItem(i);
            }
        }
        for (i, r) in self.recent_rects.iter().enumerate() {
            if r.hit(x, y) {
                return Hot::Recent(i);
            }
        }
        if let Some(d) = self.doc.as_ref() {
            if let Some(i) = d.copy_hit(x, y) {
                return Hot::CopyCode(i);
            }
        }
        if y < TITLE_H * s {
            return Hot::None;
        }
        Hot::Content
    }

    unsafe fn open_dialog(&mut self) {
        let mut buf = vec![0u16; 2048];
        let filter: Vec<u16> = "Markdown\0*.md;*.markdown;*.mdown;*.mkd;*.txt\0所有文件\0*.*\0"
            .encode_utf16()
            .collect();
        let title: Vec<u16> = "打开 Markdown".encode_utf16().collect();
        let mut ofn = OPENFILENAMEW::default();
        ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = self.hwnd;
        ofn.lpstrFilter = PCWSTR(filter.as_ptr());
        ofn.lpstrFile = PWSTR(buf.as_mut_ptr());
        ofn.nMaxFile = buf.len() as u32;
        ofn.lpstrTitle = PCWSTR(title.as_ptr());
        ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_DONTADDTORECENT | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut ofn).as_bool() {
            let end = buf.iter().position(|&c| c == 0).unwrap_or(0);
            let p = PathBuf::from(String::from_utf16_lossy(&buf[..end]));
            if p.exists() {
                self.open_file(&p);
            }
        }
    }

    unsafe fn drop_files(&mut self, hdrop: HDROP) {
        let n = DragQueryFileW(hdrop, 0xFFFFFFFF, None) as usize;
        if n == 0 {
            DragFinish(hdrop);
            return;
        }
        let len = DragQueryFileW(hdrop, 0, None) as usize;
        let mut buf = vec![0u16; len + 1];
        let got = DragQueryFileW(hdrop, 0, Some(&mut buf)) as usize;
        if got > 0 {
            let p = PathBuf::from(String::from_utf16_lossy(&buf[..got]));
            if p.exists() {
                self.open_file(&p);
            }
        }
        DragFinish(hdrop);
    }
}

// ————————————————————————————————————————————————————————————————
// 窗口过程
// ————————————————————————————————————————————————————————————————

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCCALCSIZE => {
            if wparam.0 != 0 {
                let params = lparam.0 as *mut NCCALCSIZE_PARAMS;
                if !params.is_null() && IsZoomed(hwnd).as_bool() {
                    let frame = (GetSystemMetrics(SM_CXSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER)) as i32;
                    (*params).rgrc[0].left += frame;
                    (*params).rgrc[0].top += frame;
                    (*params).rgrc[0].right -= frame;
                    (*params).rgrc[0].bottom -= frame;
                }
                return LRESULT(0);
            }
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        WM_NCHITTEST => {
            let mut p = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
            let _ = ScreenToClient(hwnd, &mut p);
            let app = match app_mut(hwnd) {
                Some(a) => a,
                None => return DefWindowProcW(hwnd, msg, wparam, lparam),
            };
            let s = app.scale;
            let zoomed = IsZoomed(hwnd).as_bool();
            if !zoomed {
                let x = p.x as f32;
                let y = p.y as f32;
                let w = app.w as f32;
                let h = app.h as f32;
                let b = 7.0 * s;
                let l = x < b;
                let r = x > w - b;
                let t = y < b;
                let bo = y > h - b;
                let ht = match (l, r, t, bo) {
                    (true, _, true, _) => HTTOPLEFT,
                    (_, true, true, _) => HTTOPRIGHT,
                    (true, _, _, true) => HTBOTTOMLEFT,
                    (_, true, _, true) => HTBOTTOMRIGHT,
                    (true, _, _, _) => HTLEFT,
                    (_, true, _, _) => HTRIGHT,
                    (_, _, true, _) => HTTOP,
                    (_, _, _, true) => HTBOTTOM,
                    _ => 0,
                };
                if ht != 0 {
                    return LRESULT(ht as isize);
                }
            }
            let hot = app.hit(p.x as f32, p.y as f32);
            if (p.y as f32) <= TITLE_H * s && hot == Hot::None && (p.x as f32) < app.btn_toc.x - 8.0 {
                return LRESULT(HTCAPTION as isize);
            }
            LRESULT(HTCLIENT as isize)
        }
        WM_GETMINMAXINFO => {
            let mmi = lparam.0 as *mut MINMAXINFO;
            if !mmi.is_null() {
                (*mmi).ptMinTrackSize = POINT { x: 520, y: 380 };
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            if let Some(app) = app_mut(hwnd) {
                app.paint();
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let s = app_mut(hwnd).map(|a| a.scale).unwrap_or(1.0);
            let mmi = lparam.0 as *mut MINMAXINFO;
            if !mmi.is_null() {
                (*mmi).ptMinTrackSize.x = (780.0 * s) as i32;
                (*mmi).ptMinTrackSize.y = (520.0 * s) as i32;
            }
            LRESULT(0)
        }
        WM_DROPFILES => {
            if let Some(app) = app_mut(hwnd) {
                app.drop_files(HDROP(wparam.0 as *mut c_void));
                app.doc = None;
                InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if let Some(app) = app_mut(hwnd) {
                let mut r = RECT::default();
                let _ = GetClientRect(hwnd, &mut r);
                app.w = r.right - r.left;
                app.h = r.bottom - r.top;
                if wparam.0 == 1 {
                    // 收起来的时候把排版和背景缓存全部放掉，内存立刻回落
                    app.doc = None;
                    app.gfx.bg = None;
                    app.gfx.bg_key = (0, 0, 0);
                    let _ = EmptyWorkingSet(GetCurrentProcess());
                }
                app.gfx.resize(app.w, app.h);
                app.chrome();
                app.clamp_scroll();
                if let Some(doc) = app.doc.as_mut() {
                    doc.key = (0, 0, 0, 0); // 触发布局
                }
                InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            if let Some(app) = app_mut(hwnd) {
                let dpi = (wparam.0 & 0xFFFF) as u32;
                app.scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
                app.gfx.fmts.clear();
                app.gfx.bg = None;
                app.gfx.bg_key = (0, 0, 0);
                let r = lparam.0 as *const RECT;
                if !r.is_null() {
                    let _ = SetWindowPos(hwnd, None, (*r).left, (*r).top, (*r).right - (*r).left, (*r).bottom - (*r).top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                app.chrome();
                InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == 2 => {
            // 一次性：欢迎页闲下来后把工作集还给系统；文档页留着，滚动不能再触页
            let _ = KillTimer(Some(hwnd), 2);
            if let Some(app) = app_mut(hwnd) {
                if app.doc.is_none() {
                    let _ = EmptyWorkingSet(GetCurrentProcess());
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if let Some(app) = app_mut(hwnd) {
                app.tick();
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if let Some(app) = app_mut(hwnd) {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                app.mouse = (x, y);
                if app.dragging_slider > 0 {
                    app.drag_slider(x);
                } else {
                    let h = app.hit(x, y);
                    if h != app.hot {
                        app.hot = h;
                        InvalidateRect(Some(hwnd), None, false);
                    }
                }
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tme).ok();
            }
            LRESULT(0)
        }
        WM_MOUSE_LEAVE => {
            if let Some(app) = app_mut(hwnd) {
                if app.hot != Hot::None {
                    app.hot = Hot::None;
                    InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let cur = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
            SetCursor(Some(cur));
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            if let Some(app) = app_mut(hwnd) {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                let h = app.hit(x, y);
                app.pressed = h;
                if let Hot::Slider(i) = h {
                    app.dragging_slider = i + 1;
                    app.drag_slider(x);
                }
                let _ = SetCapture(hwnd);
                InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(app) = app_mut(hwnd) {
                let _ = ReleaseCapture();
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                let h = app.hit(x, y);
                let pressed = app.pressed;
                let was_drag = app.dragging_slider > 0;
                app.pressed = Hot::None;
                app.dragging_slider = 0;
                if was_drag {
                    app.save_theme();
                }
                if h == pressed {
                    app.click(h);
                }
                InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if let Some(app) = app_mut(hwnd) {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32;
                app.wheel(delta);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if let Some(app) = app_mut(hwnd) {
                let vk = wparam.0 as u16;
                app.key(vk);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(app) = app_mut(hwnd) {
                if app.hwnd == hwnd {
                    PostQuitMessage(0);
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_ACTIVATE => {
            InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ————————————————————————————————————————————————————————————————
// 画笔工具
// ————————————————————————————————————————————————————————————————

unsafe fn fill_r(rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, r: R, c: Color) {
    b.SetColor(&cf(c));
    rt.FillRectangle(&rectf(r.x, r.y, r.w, r.h), b);
}

unsafe fn fill_round(rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, x: f32, y: f32, w: f32, h: f32, rad: f32, c: Color) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    b.SetColor(&cf(c));
    let r = rad.min(w * 0.5).min(h * 0.5);
    rt.FillRoundedRectangle(&rrect(x, y, w, h, r), b);
}

unsafe fn stroke_round(rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, x: f32, y: f32, w: f32, h: f32, rad: f32, c: Color, sw: f32) {
    b.SetColor(&cf(c));
    let r = rad.min(w * 0.5).min(h * 0.5);
    rt.DrawRoundedRectangle(&rrect(x, y, w, h, r), b, sw, None);
}

unsafe fn linear(rt: &ID2D1RenderTarget, stops: &[(f32, Color)], a: (f32, f32), b: (f32, f32)) -> Result<ID2D1LinearGradientBrush> {
    let s: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|(p, c)| D2D1_GRADIENT_STOP { position: *p, color: cf(*c) }).collect();
    let coll = rt.CreateGradientStopCollection(&s, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: pt(a.0, a.1), endPoint: pt(b.0, b.1) };
    rt.CreateLinearGradientBrush(&props, None, &coll)
}

unsafe fn radial(rt: &ID2D1RenderTarget, stops: &[(f32, Color)], cx: f32, cy: f32, r: f32) -> Result<ID2D1RadialGradientBrush> {
    let s: Vec<D2D1_GRADIENT_STOP> = stops.iter().map(|(p, c)| D2D1_GRADIENT_STOP { position: *p, color: cf(*c) }).collect();
    let coll = rt.CreateGradientStopCollection(&s, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
        center: pt(0.0, 0.0),
        gradientOriginOffset: pt(0.0, 0.0),
        radiusX: 1.0,
        radiusY: 1.0,
    };
    let br = rt.CreateRadialGradientBrush(&props, None, &coll)?;
    let m = Matrix3x2 { M11: r, M12: 0.0, M21: 0.0, M22: r, M31: cx, M32: cy };
    br.SetTransform(&m);
    Ok(br)
}

impl App {
    unsafe fn build_bg(&mut self, rt: &ID2D1RenderTarget, th: &Theme, w: f32, h: f32) {
        let key = (self.themes.active | ((self.theme_rev as usize) << 4), self.w, self.h);
        if self.gfx.bg.is_some() && self.gfx.bg_key == key {
            return;
        }
        let base = linear(rt, &[(0.0, th.bg_top), (1.0, th.bg_bottom)], (0.0, 0.0), (w * 0.3, h)).ok();
        let r = th.glow_size * w.max(h) * 0.62;
        let ga = radial(
            rt,
            &[(0.0, th.glow_a), (0.55, alpha(th.glow_a, th.glow_a[3] * 0.35)), (1.0, alpha(th.glow_a, 0.0))],
            th.glow_a_pos[0] * w,
            th.glow_a_pos[1] * h,
            r,
        )
        .ok();
        let gb = radial(
            rt,
            &[(0.0, th.glow_b), (0.55, alpha(th.glow_b, th.glow_b[3] * 0.3)), (1.0, alpha(th.glow_b, 0.0))],
            th.glow_b_pos[0] * w,
            th.glow_b_pos[1] * h,
            r * 0.9,
        )
        .ok();
        if let (Some(base), Some(ga), Some(gb)) = (base, ga, gb) {
            self.gfx.bg = Some(BgBrushes { base, glow_a: ga, glow_b: gb });
            self.gfx.bg_key = key;
        }
    }

    /// 一层磨砂：噪点 + 顶部高光 + 发丝边 + 外投影
    unsafe fn frost_panel(&self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, r: R, rad: f32, fill: Color, shadow: bool) {
        let s = self.scale;
        if shadow && th.shadow > 0.01 {
            let layers = 6;
            for i in 0..layers {
                let grow = (i as f32 + 1.0) * 2.2 * s;
                let a = th.shadow * 0.055 * (1.0 - i as f32 / layers as f32);
                let c = if th.is_dark() { [0.0, 0.0, 0.0, a] } else { [0.06, 0.09, 0.13, a * 0.8] };
                fill_round(rt, b, r.x - grow, r.y - grow + 2.0 * s, r.w + grow * 2.0, r.h + grow * 2.0, rad + grow, c);
            }
        }
        fill_round(rt, b, r.x, r.y, r.w, r.h, rad, fill);
        // 噪点
        if let Some(nb) = self.gfx.noise_brush.clone() {
            nb.SetOpacity(0.05 * th.frost);
            rt.FillRoundedRectangle(&rrect(r.x, r.y, r.w, r.h, rad.min(r.w * 0.5)), &nb);
        }
        // 顶部一道高光，像玻璃边缘
        let hi = if th.is_dark() { [1.0, 1.0, 1.0, 0.10] } else { [1.0, 1.0, 1.0, 0.55] };
        fill_round(rt, b, r.x + 0.5 * s, r.y + 0.5 * s, r.w - s, 1.0 * s, 0.5, hi);
        // 发丝边
        stroke_round(rt, b, r.x, r.y, r.w, r.h, rad, alpha(if th.is_dark() { [1.0, 1.0, 1.0, 1.0] } else { [0.05, 0.10, 0.15, 1.0] }, th.border_alpha), 1.0);
    }

    unsafe fn text_ui(
        &mut self,
        rt: &ID2D1RenderTarget,
        b: &ID2D1SolidColorBrush,
        s: &str,
        r: R,
        size: f32,
        weight: i32,
        color: Color,
        align: DWRITE_TEXT_ALIGNMENT,
    ) {
        let fam = self.th().font_ui.clone();
        if let Some(fmt) = self.gfx.format(&fam, size, weight, false) {
            let _ = fmt.SetTextAlignment(align);
            let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            b.SetColor(&cf(color));
            let u = text_w(s);
            rt.DrawText(&u, &fmt, &rectf(r.x, r.y, r.w, r.h), b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    unsafe fn measure_ui(&mut self, s: &str, size: f32, weight: i32) -> f32 {
        let fam = self.th().font_ui.clone();
        if let Some(fmt) = self.gfx.format(&fam, size, weight, false) {
            let u = text_w(s);
            if let Ok(l) = self.gfx.dwrite.CreateTextLayout(&u, &fmt, 4000.0, 400.0) {
                let mut m = DWRITE_TEXT_METRICS::default();
                if l.GetMetrics(&mut m).is_ok() {
                    return m.widthIncludingTrailingWhitespace;
                }
            }
        }
        s.chars().count() as f32 * size * 0.55
    }

    unsafe fn paint(&mut self) {
        let th = self.th().clone();
        let w = self.w as f32;
        let h = self.h as f32;
        let rt = match self.gfx.target() {
            Some(r) => r,
            None => return,
        };

        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(self.hwnd, &mut ps);
        if hdc.0.is_null() {
            let _ = EndPaint(self.hwnd, &ps);
            return;
        }
        let rc = RECT { left: 0, top: 0, right: self.w.max(1), bottom: self.h.max(1) };
        if let Err(e) = rt.BindDC(hdc, &rc) {
            if std::env::var_os("SCHEDA_TRACE").is_some() {
                eprintln!("BindDC failed: {e:?}");
            }
            let _ = EndPaint(self.hwnd, &ps);
            return;
        }
        rt.BeginDraw();
        let b = match self.gfx.solid(&rt, [1.0, 1.0, 1.0, 1.0]).cloned() {
            Some(b) => b,
            None => {
                let _ = rt.EndDraw(None, None);
                let _ = EndPaint(self.hwnd, &ps);
                return;
            }
        };

        self.chrome();

        // 1) 纯色底 + 极轻的磨砂噪点（不做任何渐变）
        self.build_bg(&rt, &th, w, h);
        if let Some(bg) = self.gfx.bg.as_ref() {
            rt.FillRectangle(&rectf(0.0, 0.0, w, h), &bg.base);
        }
        if let Some(nb) = self.gfx.noise_brush.clone() {
            nb.SetOpacity(0.030 * th.frost);
            rt.FillRectangle(&rectf(0.0, 0.0, w, h), &nb);
        }

        // 2) 内容
        if self.settings {
            self.paint_settings(&rt, &b, &th, w, h);
        } else if self.file.is_some() {
            self.paint_doc(&rt, &b, &th, w, h);
        } else {
            self.paint_welcome(&rt, &b, &th, w, h);
        }

        // 3) 标题栏
        self.paint_titlebar(&rt, &b, &th, w);

        // 4) 提示
        if let Some((msg, left)) = self.status.clone() {
            if left > 0.0 {
                let a = (left.min(1.0) * 0.9).clamp(0.0, 1.0);
                let tw = self.measure_ui(&msg, 12.5 * self.scale, 500) + 30.0 * self.scale;
                let cx = w * 0.5 - tw * 0.5;
                let y = h - 62.0 * self.scale;
                fill_round(&rt, &b, cx, y, tw, 32.0 * self.scale, 16.0 * self.scale, alpha(th.accent, 0.22 * a));
                stroke_round(&rt, &b, cx, y, tw, 32.0 * self.scale, 16.0 * self.scale, alpha(th.accent, 0.45 * a), 1.0);
                self.text_ui(&rt, &b, &msg, R { x: cx, y, w: tw, h: 32.0 * self.scale }, 12.5 * self.scale, 500, alpha(th.heading, a), DWRITE_TEXT_ALIGNMENT_CENTER);
            }
        }

        let ok = rt.EndDraw(None, None).is_ok();
        if std::env::var_os("SCHEDA_TRACE").is_some() {
            eprintln!("paint w={w} h={h} enddraw_ok={ok}");
        }
        let _ = EndPaint(self.hwnd, &ps);
        if !ok {
            self.gfx.rt = None;
        }
    }

    unsafe fn paint_welcome(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, w: f32, h: f32) {
        let s = self.scale;
        let cy = (h - TITLE_H * s) * 0.5 + TITLE_H * s;

        // 卡片 358*s 高，再往下挂「最近打开」；整块上下居中，窗口矮就少显示几条
        let cw = 520.0 * s;
        let chh = 168.0 * s;
        let empty = self.recent.is_empty();
        let want = self.recent.len().min(6);
        let mut rows = want;
        let mut total = 0.0;
        let mut list_h;
        loop {
            list_h = if empty { 26.0 * s + 56.0 * s } else { 26.0 * s + rows as f32 * 44.0 * s + if rows > 0 { 10.0 * s } else { 0.0 } };
            total = 358.0 * s + 18.0 * s + list_h;
            let top = cy - total * 0.5;
            if top + total <= h - 46.0 * s || rows == 0 {
                break;
            }
            rows -= 1;
        }
        let block_top = cy - total * 0.5;

        // 品牌方块（和应用图标一致）
        let m = 84.0 * s;
        let mx = (w - m) * 0.5;
        let my = block_top;
        fill_round(rt, b, mx + 1.0 * s, my + 3.0 * s, m, m, 20.0 * s, alpha([0.06, 0.09, 0.13, 1.0], 0.10));
        fill_round(rt, b, mx, my, m, m, 20.0 * s, [0.184, 0.42, 0.94, 1.0]);
        let pw = 38.0 * s;
        let ph = 51.0 * s;
        let px = mx + (m - pw) * 0.5;
        let py = my + (m - ph) * 0.5;
        fill_round(rt, b, px, py, pw, ph, 6.0 * s, [1.0, 1.0, 1.0, 1.0]);
        for i in 0..3 {
            fill_round(
                rt,
                b,
                px + 7.0 * s,
                py + 12.0 * s + i as f32 * 11.0 * s,
                if i < 2 { 24.0 } else { 15.0 } * s,
                4.0 * s,
                2.0 * s,
                [0.184, 0.42, 0.94, 1.0],
            );
        }

        // 大字标
        self.text_ui(rt, b, "Scheda", R { x: 0.0, y: my + m + 10.0 * s, w, h: 66.0 * s }, 44.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_CENTER);
        self.text_ui(
            rt,
            b,
            "把 Markdown 读成它该有的样子",
            R { x: 0.0, y: my + m + 72.0 * s, w, h: 24.0 * s },
            13.0 * s,
            400,
            alpha(th.muted, 0.95),
            DWRITE_TEXT_ALIGNMENT_CENTER,
        );

        // 玻璃卡片
        let cr = R { x: (w - cw) * 0.5, y: my + m + 106.0 * s, w: cw, h: chh };
        let fillc = if th.is_dark() {
            alpha([1.0, 1.0, 1.0, 1.0], 0.06)
        } else {
            theme::over([1.0, 1.0, 1.0, 1.0], th.bg_top, 0.96)
        };
        self.frost_panel(rt, b, th, cr, th.radius, fillc, true);

        // 卡片内的小图标：一页纸
        let ix = cr.x + 28.0 * s;
        let iy = cr.y + 28.0 * s;
        fill_round(rt, b, ix, iy, 38.0 * s, 46.0 * s, 9.0 * s, alpha(th.accent, 0.14));
        stroke_round(rt, b, ix, iy, 38.0 * s, 46.0 * s, 9.0 * s, alpha(th.accent, 0.45), 1.0);
        for i in 0..3 {
            fill_round(
                rt,
                b,
                ix + 10.0 * s,
                iy + 12.0 * s + i as f32 * 9.0 * s,
                if i < 2 { 18.0 } else { 11.0 } * s,
                2.2 * s,
                1.1 * s,
                alpha(th.accent, 0.85),
            );
        }

        self.text_ui(
            rt,
            b,
            "把 .md 文件拖进来",
            R { x: ix + 54.0 * s, y: iy - 3.0 * s, w: cr.w - 90.0 * s, h: 28.0 * s },
            18.0 * s,
            600,
            th.heading,
            DWRITE_TEXT_ALIGNMENT_LEADING,
        );
        self.text_ui(
            rt,
            b,
            "或者按 Ctrl+O 选一个文件 — 双击就开，看完就关",
            R { x: ix + 54.0 * s, y: iy + 24.0 * s, w: cr.w - 90.0 * s, h: 20.0 * s },
            12.0 * s,
            400,
            th.muted,
            DWRITE_TEXT_ALIGNMENT_LEADING,
        );

        // 快捷键 chips：先量总宽再居中，四个 pill 在卡片里对称铺开
        let chips = [("Ctrl+O", "打开"), ("Ctrl+E", "设置"), ("T", "目录"), ("Ctrl+滚轮", "缩放")];
        let mut widths = [0.0f32; 4];
        let mut total = 0.0f32;
        for (i, (k, label)) in chips.iter().enumerate() {
            widths[i] = self.measure_ui(k, 11.5 * s, 600) + self.measure_ui(label, 11.5 * s, 400) + 30.0 * s;
            total += widths[i];
        }
        let gapc = ((cr.w - 56.0 * s - total) / 3.0).clamp(8.0 * s, 26.0 * s);
        total += gapc * 3.0;
        let mut x = cr.x + (cr.w - total).max(0.0) * 0.5;
        let cy2 = cr.y + chh - 52.0 * s;
        for (i, (k, label)) in chips.iter().enumerate() {
            let kw = self.measure_ui(k, 11.5 * s, 600);
            let lw = self.measure_ui(label, 11.5 * s, 400);
            let pwi = widths[i];
            fill_round(rt, b, x, cy2, pwi, 26.0 * s, 13.0 * s, alpha(if th.is_dark() { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] }, if th.is_dark() { 0.06 } else { 0.05 }));
            self.text_ui(rt, b, k, R { x: x + 11.0 * s, y: cy2, w: kw + 2.0, h: 26.0 * s }, 11.5 * s, 600, th.accent, DWRITE_TEXT_ALIGNMENT_LEADING);
            self.text_ui(rt, b, label, R { x: x + 13.0 * s + kw, y: cy2, w: lw + 2.0, h: 26.0 * s }, 11.5 * s, 400, th.muted, DWRITE_TEXT_ALIGNMENT_LEADING);
            x += pwi + gapc;
        }

        // —— 最近打开
        let lt = cr.y + chh + 18.0 * s;
        self.recent_rects.clear();
        self.text_ui(rt, b, "最近打开", R { x: cr.x + 16.0 * s, y: lt, w: 180.0 * s, h: 18.0 * s }, 10.5 * s, 600, alpha(th.muted, 0.95), DWRITE_TEXT_ALIGNMENT_LEADING);
        if !empty {
            self.text_ui(
                rt,
                b,
                "点一下就开",
                R { x: cr.x + cr.w - 120.0 * s, y: lt, w: 104.0 * s, h: 18.0 * s },
                10.0 * s,
                400,
                alpha(th.muted, 0.6),
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            );
        }
        if empty {
            let er = R { x: cr.x, y: lt + 26.0 * s, w: cr.w, h: 56.0 * s };
            fill_round(rt, b, er.x, er.y, er.w, er.h, 12.0 * s, alpha(if th.is_dark() { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] }, 0.03));
            stroke_round(rt, b, er.x, er.y, er.w, er.h, 12.0 * s, alpha(th.text, 0.06), 1.0);
            self.text_ui(
                rt,
                b,
                "还没有打开过文件 · 拖一个 .md 进来，或者按 Ctrl+O",
                R { x: er.x + 16.0 * s, y: er.y, w: er.w - 32.0 * s, h: er.h },
                11.0 * s,
                400,
                alpha(th.muted, 0.9),
                DWRITE_TEXT_ALIGNMENT_CENTER,
            );
        } else {
            for i in 0..rows {
                let p = self.recent[i].clone();
                let r = R { x: cr.x, y: lt + 26.0 * s + i as f32 * 44.0 * s, w: cr.w, h: 40.0 * s };
                self.recent_rects.push(r);
                if i > 0 {
                    fill_r(rt, b, R { x: r.x + 16.0 * s, y: r.y - 2.0 * s, w: r.w - 32.0 * s, h: 1.0 }, alpha(th.text, 0.045));
                }
                if self.hot == Hot::Recent(i) {
                    fill_round(rt, b, r.x, r.y, r.w, r.h, 10.0 * s, alpha(th.accent, 0.09));
                }
                // 小纸页图标
                let icx = r.x + 16.0 * s;
                let icy = r.cy() - 13.0 * s;
                fill_round(rt, b, icx, icy, 26.0 * s, 26.0 * s, 7.0 * s, alpha(th.accent, 0.10));
                fill_round(rt, b, icx + 8.0 * s, icy + 5.0 * s, 10.0 * s, 15.0 * s, 2.5 * s, [1.0, 1.0, 1.0, 1.0]);
                for k in 0..3 {
                    fill_round(rt, b, icx + 10.0 * s, icy + 8.0 * s + k as f32 * 4.0 * s, if k < 2 { 6.0 } else { 4.0 } * s, 1.5 * s, 0.75 * s, alpha(th.accent, 0.8));
                }
                let pb = PathBuf::from(&p);
                let mut name = pb.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| p.clone());
                if name.chars().count() > 26 {
                    name = name.chars().take(24).collect::<String>() + "…";
                }
                let dir = pb.parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                self.text_ui(rt, b, &name, R { x: icx + 36.0 * s, y: r.y + 5.0 * s, w: r.w - 62.0 * s, h: 17.0 * s }, 12.5 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
                let mut dshort = if dir.chars().count() > 58 {
                    let head: String = dir.chars().take(16).collect();
                    let tail: String = dir.chars().skip(dir.chars().count() - 38).collect();
                    format!("{}…{}", head, tail)
                } else {
                    dir.clone()
                };
                if dshort.is_empty() {
                    dshort = "本地文件".into();
                }
                self.text_ui(rt, b, &dshort, R { x: icx + 36.0 * s, y: r.y + 22.0 * s, w: r.w - 62.0 * s, h: 15.0 * s }, 9.5 * s, 400, alpha(th.muted, 0.9), DWRITE_TEXT_ALIGNMENT_LEADING);
            }
        }

        self.text_ui(
            rt,
            b,
            "小方工作室 · 零联网 · 零常驻",
            R { x: 0.0, y: h - 40.0 * s, w, h: 20.0 * s },
            11.0 * s,
            400,
            alpha(th.muted, 0.65),
            DWRITE_TEXT_ALIGNMENT_CENTER,
        );
    }

    unsafe fn paint_titlebar(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, w: f32) {
        let s = self.scale;
        let thh = TITLE_H * s;

        // 玻璃条
        let tint = if th.is_dark() {
            [1.0, 1.0, 1.0, 0.035]
        } else {
            theme::over([1.0, 1.0, 1.0, 1.0], th.bg_top, 0.92)
        };
        fill_r(rt, b, R { x: 0.0, y: 0.0, w, h: thh }, tint);
        // 底部发丝线
        fill_r(
            rt,
            b,
            R { x: 0.0, y: thh - 1.0, w, h: 1.0 },
            alpha(if th.is_dark() { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] }, if th.is_dark() { 0.07 } else { 0.07 }),
        );

        // logo
        let lx = 18.0 * s;
        let ls = 27.0 * s;
        let ly = (thh - ls) * 0.5;
        if let Ok(g) = linear(rt, &[(0.0, th.accent), (1.0, th.accent2)], (lx, ly), (lx + ls, ly + ls)) {
            rt.FillRoundedRectangle(&rrect(lx, ly, ls, ls, 8.0 * s), &g);
        }
        // 白色纸页记号
        let px = lx + 7.0 * s;
        let py = ly + 6.0 * s;
        fill_round(rt, b, px, py, 13.0 * s, 15.0 * s, 3.0 * s, [1.0, 1.0, 1.0, 0.92]);
        for i in 0..3 {
            fill_round(rt, b, px + 2.6 * s, py + 3.4 * s + i as f32 * 3.2 * s, (8.0 - i as f32 * 2.2) * s, 1.3 * s, 0.6 * s, alpha(th.accent, 0.55));
        }

        // 标题
        let tx = lx + ls + 11.0 * s;
        let bar_x = if self.btn_toc.w > 0.0 {
            self.btn_toc.x
        } else if self.btn_theme.w > 0.0 {
            self.btn_theme.x
        } else {
            self.btn_min.x
        };
        let maxw = (bar_x - tx - 14.0 * s).max(80.0);
        let title = self.title();
        self.text_ui(rt, b, &title, R { x: tx, y: 9.0 * s, w: maxw, h: 22.0 * s }, 14.0 * s, 500, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        let sub = self.subtitle();
        self.text_ui(rt, b, &sub, R { x: tx, y: 28.0 * s, w: maxw, h: 16.0 * s }, 10.0 * s, 400, alpha(th.muted, 0.8), DWRITE_TEXT_ALIGNMENT_LEADING);

        // 右侧按钮
        self.win_btn(rt, b, th, self.btn_toc, self.hot == Hot::Toc, 3);
        self.win_btn(rt, b, th, self.btn_theme, self.hot == Hot::Theme, 4);
        self.win_btn(rt, b, th, self.btn_min, self.hot == Hot::Min, 0);
        self.win_btn(rt, b, th, self.btn_max, self.hot == Hot::Max, 1);
        self.win_btn(rt, b, th, self.btn_close, self.hot == Hot::Close || self.hot == Hot::PanelClose, 2);

        // 阅读进度
        if let Some(doc) = self.doc.as_ref() {
            let max = self.max_scroll().max(1.0);
            let p = (self.scroll / max).clamp(0.0, 1.0);
            let _ = doc;
            if p > 0.005 {
                if let Ok(g) = linear(rt, &[(0.0, th.accent), (1.0, th.accent2)], (0.0, 0.0), (w * 0.4, 0.0)) {
                    rt.FillRoundedRectangle(&rrect(0.0, thh - 2.0 * s, (w * p).max(3.0), 2.0 * s, 1.0 * s), &g);
                }
            }
        }
    }

    unsafe fn win_btn(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, r: R, hot: bool, kind: u8) {
        if r.w <= 0.0 {
            return;
        }
        let s = self.scale;
        if hot {
            let c = if kind == 2 {
                if th.is_dark() { [0.86, 0.20, 0.24, 0.85] } else { [0.90, 0.16, 0.22, 1.0] }
            } else if th.is_dark() {
                [1.0, 1.0, 1.0, 0.09]
            } else {
                [0.0, 0.0, 0.0, 0.07]
            };
            fill_round(rt, b, r.x, r.y, r.w, r.h, 9.0 * s, c);
        }
        let ic = if hot && kind == 2 { [1.0, 1.0, 1.0, 1.0] } else { alpha(th.text, 0.78) };
        let cx = r.cx();
        let cy = r.cy();
        match kind {
            0 => {
                fill_round(rt, b, cx - 5.5 * s, cy - 0.75 * s, 11.0 * s, 1.5 * s, 0.75 * s, ic);
            }
            1 => {
                let z = IsZoomed(self.hwnd).as_bool();
                if z {
                    stroke_round(rt, b, cx - 6.0 * s, cy - 5.0 * s, 9.0 * s, 9.0 * s, 1.0 * s, ic, 1.2);
                    fill_round(rt, b, cx - 3.5 * s, cy - 2.5 * s, 9.0 * s, 8.0 * s, 1.0 * s, alpha(th.bg_top, 0.9));
                    stroke_round(rt, b, cx - 3.5 * s, cy - 2.5 * s, 9.0 * s, 9.0 * s, 1.0 * s, ic, 1.2);
                } else {
                    stroke_round(rt, b, cx - 5.0 * s, cy - 5.0 * s, 10.0 * s, 10.0 * s, 1.5 * s, ic, 1.2);
                }
            }
            2 => {
                let a = std::f32::consts::FRAC_PI_4;
                rt.SetTransform(&rot_about(a, cx, cy));
                fill_round(rt, b, cx - 6.0 * s, cy - 0.8 * s, 12.0 * s, 1.6 * s, 0.8 * s, ic);
                rt.SetTransform(&rot_about(-a, cx, cy));
                fill_round(rt, b, cx - 6.0 * s, cy - 0.8 * s, 12.0 * s, 1.6 * s, 0.8 * s, ic);
                rt.SetTransform(&ident());
            }
            3 => {
                for i in 0..3 {
                    fill_round(rt, b, cx - 6.0 * s, cy - 5.0 * s + i as f32 * 4.2 * s, 2.8 * s, 2.8 * s, 1.4 * s, alpha(th.accent, 0.85));
                    fill_round(rt, b, cx - 1.5 * s, cy - 4.3 * s + i as f32 * 4.2 * s, 7.5 * s, 1.4 * s, 0.7 * s, ic);
                }
            }
            4 => {
                // 主题：一个小色点 + 文字
                let dot = 11.0 * s;
                if let Ok(g) = linear(rt, &[(0.0, th.accent), (1.0, th.accent2)], (cx - 22.0 * s, cy), (cx - 22.0 * s + dot, cy + dot)) {
                    rt.FillRoundedRectangle(&rrect(cx - 25.0 * s, cy - dot * 0.5, dot, dot, dot * 0.5), &g);
                }
                self.text_ui(
                    rt,
                    b,
                    "主题",
                    R { x: cx - 5.0 * s, y: r.y, w: 30.0 * s, h: r.h },
                    11.5 * s,
                    500,
                    if hot { th.heading } else { alpha(th.text, 0.82) },
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                );
            }
            _ => {}
        }
    }
}

fn rot_about(angle: f32, cx: f32, cy: f32) -> Matrix3x2 {
    let c = angle.cos();
    let s = angle.sin();
    Matrix3x2 {
        M11: c,
        M12: s,
        M21: -s,
        M22: c,
        M31: cx - c * cx + s * cy,
        M32: cy - s * cx - c * cy,
    }
}

// ————————————————————————————————————————————————————————————————
// 文档 / 交互（渲染引擎阶段二填充）
// ————————————————————————————————————————————————————————————————

impl App {
    unsafe fn paint_doc(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, w: f32, h: f32) {
        self.paint_document(rt, b, th, w, h);
    }

    unsafe fn paint_settings(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, w: f32, h: f32) {
        let s = self.scale;
        let m = 26.0 * s;
        let top = TITLE_H * s + 22.0 * s;
        let inner_w = w - m * 2.0;
        let card_fill = if th.is_dark() { alpha([1.0, 1.0, 1.0, 1.0], 0.06) } else { theme::over([1.0, 1.0, 1.0, 1.0], th.bg_top, 0.97) };

        // 标题
        self.text_ui(rt, b, "设置", R { x: m, y: top, w: 260.0 * s, h: 34.0 * s }, 21.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        self.text_ui(
            rt,
            b,
            "外观与排版 · 改动即时生效，自动写进 settings.json",
            R { x: m, y: top + 34.0 * s, w: inner_w - 70.0 * s, h: 18.0 * s },
            10.5 * s,
            400,
            alpha(th.muted, 0.9),
            DWRITE_TEXT_ALIGNMENT_LEADING,
        );

        // 右上关闭
        let xr = R { x: w - m - 32.0 * s, y: top - 4.0 * s, w: 32.0 * s, h: 32.0 * s };
        self.panel_x = xr;
        let hot_x = self.hot == Hot::PanelClose;
        if hot_x {
            fill_round(rt, b, xr.x, xr.y, xr.w, xr.h, 9.0 * s, alpha(th.text, 0.07));
        }
        let ic = alpha(th.text, if hot_x { 0.9 } else { 0.5 });
        let ang = std::f32::consts::FRAC_PI_4;
        rt.SetTransform(&rot_about(ang, xr.cx(), xr.cy()));
        fill_round(rt, b, xr.cx() - 5.0 * s, xr.cy() - 0.7 * s, 10.0 * s, 1.4 * s, 0.7 * s, ic);
        rt.SetTransform(&rot_about(-ang, xr.cx(), xr.cy()));
        fill_round(rt, b, xr.cx() - 5.0 * s, xr.cy() - 0.7 * s, 10.0 * s, 1.4 * s, 0.7 * s, ic);
        rt.SetTransform(&ident());

        let cy = top + 68.0 * s;

        // —— 左：主题预设
        let avail_h = (h - cy - 22.0 * s).max(320.0 * s);
        let lw = 292.0 * s;
        let rh = 44.0 * s;
        let lh = (64.0 * s + 7.0 * (rh + 6.0 * s) + 12.0 * s).min(avail_h);
        let lr = R { x: m, y: cy, w: lw, h: lh };
        self.frost_panel(rt, b, th, lr, th.radius, card_fill, true);
        let lp = 20.0 * s;
        let lcx = lr.x + lp;
        let lcw = lr.w - lp * 2.0;
        self.text_ui(rt, b, "主题", R { x: lcx, y: lr.y + 18.0 * s, w: lcw, h: 22.0 * s }, 13.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        self.text_ui(rt, b, "全部浅色 · 选中即换", R { x: lcx, y: lr.y + 40.0 * s, w: lcw, h: 16.0 * s }, 9.5 * s, 400, alpha(th.muted, 0.85), DWRITE_TEXT_ALIGNMENT_LEADING);

        self.preset_rects.clear();
        let presets: Vec<(String, Color, Color)> = self.themes.all.iter().map(|p| (p.name.clone(), p.accent, p.accent2)).collect();
        let list_top = lr.y + 60.0 * s;
        let list_h = lr.h - 68.0 * s;
        let cnt = presets.len().max(1) as f32;
        let _ = (list_h, cnt);
        for (i, (pname, pac, pac2)) in presets.iter().enumerate() {
            let pr = R { x: lcx, y: list_top + i as f32 * (rh + 6.0 * s), w: lcw, h: rh };
            self.preset_rects.push(pr);
            if pr.y + pr.h > lr.y + lr.h - 6.0 * s {
                continue;
            }
            let on = i == self.themes.active;
            let hot = self.hot == Hot::Preset(i);
            if on || hot {
                fill_round(rt, b, pr.x, pr.y, pr.w, pr.h, 10.0 * s, if on { alpha(th.accent, 0.12) } else { alpha(th.text, 0.05) });
            }
            if on {
                stroke_round(rt, b, pr.x, pr.y, pr.w, pr.h, 10.0 * s, alpha(th.accent, 0.38), 1.0);
                fill_round(rt, b, pr.x, pr.y + 9.0 * s, 3.0 * s, pr.h - 18.0 * s, 1.5 * s, alpha(th.accent, 0.9));
            }
            let d = 14.0 * s;
            let dx = pr.x + 16.0 * s;
            let dy = pr.cy() - d * 0.5;
            fill_round(rt, b, dx, dy, d, d, d * 0.5, *pac);
            fill_round(rt, b, dx + d * 0.5, dy + d * 0.5, d * 0.5, d * 0.5, d * 0.25, *pac2);
            self.text_ui(
                rt,
                b,
                pname,
                R { x: dx + d + 10.0 * s, y: pr.y, w: pr.w - (d + 46.0 * s), h: pr.h },
                11.5 * s,
                if on { 600 } else { 500 },
                if on { th.heading } else { alpha(th.text, 0.82) },
                DWRITE_TEXT_ALIGNMENT_LEADING,
            );
            if on {
                let kd = 5.0 * s;
                fill_round(rt, b, pr.x + pr.w - 22.0 * s, pr.cy() - kd, kd * 2.0, kd * 2.0, kd, alpha(th.accent, 0.9));
            }
        }

        // —— 左：预览卡（高度只给窗口剩下的部分，不够就少画几行，绝不出界）
        let py = lr.y + lr.h + 14.0 * s;
        let ph = h - py - 22.0 * s;
        if ph >= 148.0 * s {
            let prr = R { x: m, y: py, w: lw, h: ph };
            self.frost_panel(rt, b, th, prr, th.radius, card_fill, true);
            let pb = prr.y + prr.h - 10.0 * s;
            self.text_ui(rt, b, "预览", R { x: lcx, y: prr.y + 16.0 * s, w: lcw, h: 20.0 * s }, 11.5 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
            let ty = prr.y + 44.0 * s;
            self.text_ui(rt, b, "今晚的月亮", R { x: lcx, y: ty, w: lcw, h: 30.0 * s }, 17.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
            if ty + 36.0 * s + 22.0 * s < pb {
                self.text_ui(
                    rt,
                    b,
                    "小方把 Markdown 读成它该有的样子，",
                    R { x: lcx, y: ty + 36.0 * s, w: lcw, h: 22.0 * s },
                    11.0 * s,
                    400,
                    th.text,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                );
            }
            if ty + 58.0 * s + 22.0 * s < pb {
                self.text_ui(
                    rt,
                    b,
                    "零常驻 · 零联网 · 冷启动几十毫秒。",
                    R { x: lcx, y: ty + 58.0 * s, w: lcw, h: 22.0 * s },
                    11.0 * s,
                    400,
                    alpha(th.text, 0.86),
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                );
            }
            let pill = R { x: lcx, y: ty + 88.0 * s, w: 78.0 * s, h: 22.0 * s };
            if pill.y + pill.h + 6.0 * s < pb {
                fill_round(rt, b, pill.x, pill.y, pill.w, pill.h, 4.0 * s, th.code_bg);
                self.text_ui(rt, b, "scheda", pill, 10.5 * s, 500, th.code_text, DWRITE_TEXT_ALIGNMENT_CENTER);
                fill_round(rt, b, lcx + 92.0 * s, ty + 88.0 * s, 44.0 * s, 22.0 * s, 4.0 * s, th.quote_bg);
                self.text_ui(rt, b, "#44", R { x: lcx + 92.0 * s, y: ty + 88.0 * s, w: 44.0 * s, h: 22.0 * s }, 10.5 * s, 500, th.accent, DWRITE_TEXT_ALIGNMENT_CENTER);
            }
            let qy = ty + 122.0 * s;
            if qy + 44.0 * s < pb {
                fill_round(rt, b, lcx, qy, 3.0 * s, 38.0 * s, 1.5 * s, alpha(th.accent, 0.85));
                self.text_ui(rt, b, "引用块也是这个味道。", R { x: lcx + 14.0 * s, y: qy - 2.0 * s, w: lcw - 18.0 * s, h: 24.0 * s }, 10.5 * s, 400, alpha(th.muted, 1.0), DWRITE_TEXT_ALIGNMENT_LEADING);
                self.text_ui(rt, b, "圆角、磨砂、字号、段距全都改得动。", R { x: lcx + 14.0 * s, y: qy + 18.0 * s, w: lcw - 18.0 * s, h: 24.0 * s }, 10.5 * s, 400, alpha(th.muted, 0.9), DWRITE_TEXT_ALIGNMENT_LEADING);
            }
        }

        // —— 右列
        let rx = m + lw + 16.0 * s;
        let rw = (w - m - rx).max(240.0 * s);
        let rp = 22.0 * s;
        let rcx = rx + rp;
        let rcw = rw - rp * 2.0;

        // 卡 1：强调色 + 形态（按可用高度分配）
        let gapv = 12.0 * s;
        let c3h = 36.0 * s;
        let rem = (avail_h - gapv * 2.0 - c3h).max(240.0 * s);
        // 强调色卡按内容定高（两列 7 个滑杆），省下的高度全给字体卡，窗口再矮也不会画到屏幕外
        let c1h = (298.0 * s).min((rem - 132.0 * s).max(240.0 * s));
        let c1 = R { x: rx, y: cy, w: rw, h: c1h };
        self.frost_panel(rt, b, th, c1, th.radius, card_fill, true);
        self.text_ui(rt, b, "强调色", R { x: rcx, y: c1.y + 18.0 * s, w: rcw, h: 20.0 * s }, 12.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        self.text_ui(rt, b, theme::to_hex(th.accent).to_uppercase().as_str(), R { x: rcx + rcw - 70.0 * s, y: c1.y + 18.0 * s, w: 70.0 * s, h: 20.0 * s }, 9.5 * s, 500, alpha(th.muted, 0.9), DWRITE_TEXT_ALIGNMENT_TRAILING);

        self.swatch_rects.clear();
        let sw: [&str; 6] = ["#0E8FB8", "#5851D6", "#E05A8C", "#2AA97B", "#C98A24", "#D4574E"];
        let cur_hex = theme::to_hex(th.accent).to_uppercase();
        let d = 22.0 * s;
        let sg = (rcw - d * 6.0) / 5.0;
        for (i, hx) in sw.iter().enumerate() {
            let sr = R { x: rcx + i as f32 * (d + sg), y: c1.y + 46.0 * s, w: d, h: d };
            self.swatch_rects.push(R { x: sr.x - 6.0 * s, y: sr.y - 6.0 * s, w: sr.w + 12.0 * s, h: sr.h + 12.0 * s });
            if let Some(c) = theme::parse_hex(hx) {
                if hx.to_uppercase() == cur_hex {
                    stroke_round(rt, b, sr.x - 5.0 * s, sr.y - 5.0 * s, sr.w + 10.0 * s, sr.h + 10.0 * s, (sr.w + 10.0 * s) * 0.5, alpha(th.heading, 0.55), 1.5);
                }
                if self.hot == Hot::Swatch(i) {
                    stroke_round(rt, b, sr.x - 5.0 * s, sr.y - 5.0 * s, sr.w + 10.0 * s, sr.h + 10.0 * s, (sr.w + 10.0 * s) * 0.5, alpha(th.accent, 0.35), 1.5);
                }
                fill_round(rt, b, sr.x, sr.y, sr.w, sr.h, d * 0.5, c);
            }
        }

        fill_r(rt, b, R { x: rcx, y: c1.y + 88.0 * s, w: rcw, h: 1.0 }, alpha(th.text, 0.07));
        self.text_ui(rt, b, "形态", R { x: rcx, y: c1.y + 102.0 * s, w: rcw, h: 20.0 * s }, 12.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);

        let items: [(String, String, f32); 7] = [
            ("圆角".into(), format!("{:.0}", th.radius), (th.radius - 8.0) / 28.0),
            ("磨砂".into(), format!("{:.0}%", th.frost * 100.0), th.frost),
            ("字号".into(), format!("{:.0}", th.font_size), (th.font_size - 13.0) / 9.0),
            ("行距".into(), format!("{:.2}", th.line_height), (th.line_height - 1.35) / 1.05),
            ("段距".into(), format!("{:.0}", th.para_gap), (th.para_gap - 2.0) / 42.0),
            ("阴影".into(), format!("{:.0}%", th.shadow * 100.0), (th.shadow - 0.06) / 0.39),
            ("纸面".into(), format!("{:.0}%", th.sheet_alpha * 100.0), ((th.sheet_alpha - 0.82) / 0.18).clamp(0.0, 1.0)),
        ];
        // 七个滑杆排成两列（左 4 右 3），矮窗口也不会把滑杆挤到卡片外面
        let colw = (rcw - 48.0 * s) * 0.5;
        let rowh = 40.0 * s;
        for (i, (name, val, t)) in items.iter().enumerate() {
            let col = if i < 4 { 0.0 } else { 1.0 };
            let row = if i < 4 { i } else { i - 4 };
            let cx0 = rcx + col * (colw + 48.0 * s);
            let ty = c1.y + 128.0 * s + row as f32 * rowh;
            self.text_ui(rt, b, name, R { x: cx0, y: ty, w: 70.0 * s, h: 16.0 * s }, 10.5 * s, 500, alpha(th.text, 0.8), DWRITE_TEXT_ALIGNMENT_LEADING);
            self.text_ui(rt, b, val, R { x: cx0 + colw - 56.0 * s, y: ty, w: 56.0 * s, h: 16.0 * s }, 10.0 * s, 500, th.accent, DWRITE_TEXT_ALIGNMENT_TRAILING);
            let tr = R { x: cx0, y: ty + 20.0 * s, w: colw, h: 4.0 * s };
            self.slider_rects[i] = R { x: tr.x - 8.0 * s, y: tr.y - 11.0 * s, w: tr.w + 16.0 * s, h: 26.0 * s };
            fill_round(rt, b, tr.x, tr.y, tr.w, tr.h, 2.0 * s, alpha(th.text, 0.09));
            let tw = (tr.w * t.clamp(0.0, 1.0)).max(14.0 * s);
            // 纯色填充，不用渐变：浅色主题上渐变容易显脏
            fill_round(rt, b, tr.x, tr.y, tw, tr.h, 2.0 * s, th.accent);
            let kd = 15.0 * s;
            let kx = tr.x + tw - kd * 0.5;
            let ky = tr.y + tr.h * 0.5 - kd * 0.5;
            fill_round(rt, b, kx + 0.5 * s, ky + 1.5 * s, kd, kd, kd * 0.5, alpha([0.06, 0.09, 0.13, 1.0], 0.12));
            fill_round(rt, b, kx, ky, kd, kd, kd * 0.5, [1.0, 1.0, 1.0, 1.0]);
            stroke_round(rt, b, kx, ky, kd, kd, kd * 0.5, alpha(th.accent, 0.5), 1.3);
        }

        // 卡 2：字体
        let c2y = cy + c1h + gapv;
        let c2h = (avail_h - gapv * 2.0 - c3h - c1h).max(150.0 * s);
        let c2 = R { x: rx, y: c2y, w: rw, h: c2h };
        if c2y + c2h <= h - 4.0 * s {
            self.frost_panel(rt, b, th, c2, th.radius, card_fill, true);
            self.text_ui(rt, b, "字体", R { x: rcx, y: c2.y + 18.0 * s, w: rcw, h: 20.0 * s }, 12.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
            self.text_ui(rt, b, "点一下换下一款", R { x: rcx + rcw - 110.0 * s, y: c2.y + 18.0 * s, w: 110.0 * s, h: 20.0 * s }, 9.5 * s, 400, alpha(th.muted, 0.85), DWRITE_TEXT_ALIGNMENT_TRAILING);
            let rows: [(&str, usize); 3] = [("界面", 0), ("正文", 1), ("代码", 2)];
            let f_gap = ((c2h - 58.0 * s) / 3.0).clamp(34.0 * s, 48.0 * s);
            for (i, (label, _)) in rows.iter().enumerate() {
                let fr = R { x: rcx, y: c2.y + 46.0 * s + i as f32 * f_gap, w: rcw, h: 34.0 * s };
                self.font_rects[i] = fr;
                if self.hot == Hot::Font(i as u8) {
                    fill_round(rt, b, fr.x, fr.y, fr.w, fr.h, 8.0 * s, alpha(th.text, 0.05));
                }
                self.text_ui(rt, b, label, R { x: fr.x + 4.0 * s, y: fr.y, w: 46.0 * s, h: fr.h }, 10.0 * s, 500, alpha(th.muted, 0.95), DWRITE_TEXT_ALIGNMENT_LEADING);
                let fam = match i {
                    0 => th.font_ui.clone(),
                    1 => th.font_text.clone(),
                    _ => th.font_mono.clone(),
                };
                self.text_ui(rt, b, &fam, R { x: fr.x + 58.0 * s, y: fr.y, w: fr.w - 100.0 * s, h: fr.h }, 11.0 * s, 500, th.text, DWRITE_TEXT_ALIGNMENT_LEADING);
                self.text_ui(rt, b, "›", R { x: fr.x + fr.w - 26.0 * s, y: fr.y, w: 22.0 * s, h: fr.h }, 13.0 * s, 500, alpha(th.muted, 0.7), DWRITE_TEXT_ALIGNMENT_CENTER);
            }
        } else {
            self.font_rects = [R::default(); 3];
        }

        // 卡 3：按钮
        let c3y = c2y + c2h + gapv;
        let bgap = 10.0 * s;
        let bw = (rw - bgap) * 0.5;
        self.chip_open = R { x: rx, y: c3y, w: bw, h: 36.0 * s };
        self.chip_reset = R { x: rx + bw + bgap, y: c3y, w: bw, h: 36.0 * s };
        self.chip_themes = R::default();
        if c3y + 36.0 * s <= h - 6.0 * s {
            for (chip, label, hot) in [
                (self.chip_open, "打开主题文件夹", self.hot == Hot::OpenThemes),
                (self.chip_reset, "恢复当前预设", self.hot == Hot::ResetTheme),
            ] {
                if hot {
                    fill_round(rt, b, chip.x, chip.y, chip.w, chip.h, 9.0 * s, alpha(th.accent, 0.14));
                } else {
                    fill_round(rt, b, chip.x, chip.y, chip.w, chip.h, 9.0 * s, card_fill);
                }
                stroke_round(rt, b, chip.x, chip.y, chip.w, chip.h, 9.0 * s, alpha(th.text, 0.08), 1.0);
                self.text_ui(rt, b, label, R { x: chip.x, y: chip.y, w: chip.w, h: chip.h }, 10.5 * s, 500, alpha(th.text, 0.85), DWRITE_TEXT_ALIGNMENT_CENTER);
            }
        }
        self.panel_rect = R { x: 0.0, y: TITLE_H * s, w, h: h - TITLE_H * s };
    }
    unsafe fn tick(&mut self) {
        let mut busy = false;
        let d = self.scroll_t - self.scroll;
        if d.abs() > 0.4 {
            self.scroll += d * 0.24;
            busy = true;
        } else if d.abs() > 0.001 {
            self.scroll = self.scroll_t;
        }
        if let Some((_, left)) = &mut self.status {
            if *left > 0.0 {
                *left -= 0.008;
                busy = true;
                if *left < 0.0 {
                    *left = 0.0;
                }
            }
        }
        InvalidateRect(Some(self.hwnd), None, false);
        if !busy {
            KillTimer(Some(self.hwnd), 1).ok();
        }
    }

    unsafe fn clamp_scroll(&mut self) {
        let max = self.max_scroll();
        self.scroll_t = self.scroll_t.clamp(0.0, max);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    fn max_scroll(&self) -> f32 {
        match &self.doc {
            Some(d) => (d.height + 34.0 * self.scale + 24.0 * self.scale - d.view_h).max(0.0),
            None => 0.0,
        }
    }

    unsafe fn wheel(&mut self, delta: f32) {
        let max = self.max_scroll();
        let step = delta * 1.35 * self.scale;
        self.scroll_t = (self.scroll_t - step).clamp(0.0, max);
        SetTimer(Some(self.hwnd), 1, 8, None);
    }

    unsafe fn drag_slider(&mut self, x: f32) {
        let i = (self.dragging_slider - 1) as usize;
        if i >= self.slider_rects.len() {
            return;
        }
        let r = self.slider_rects[i];
        let t = ((x - r.x) / r.w.max(1.0)).clamp(0.0, 1.0);
        match i {
            0 => {
                let v = 8.0 + t * 28.0;
                self.set_override("radius", serde_json::json!(v as i32));
            }
            1 => {
                self.set_override("frost", serde_json::json!(t));
            }
            2 => {
                let v = 13.0 + t * 9.0;
                self.set_override("font_size", serde_json::json!(v as i32));
            }
            3 => {
                let v = 1.35 + t * 1.05;
                self.set_override("line_height", serde_json::json!((v * 100.0).round() / 100.0));
            }
            4 => {
                let v = 2.0 + t * 42.0;
                self.set_override("para_gap", serde_json::json!(v.round() as i32));
            }
            5 => {
                let v = 0.06 + t * 0.39;
                self.set_override("shadow", serde_json::json!((v * 100.0).round() / 100.0));
            }
            _ => {
                let v = 0.82 + t * 0.18;
                self.set_override("sheet_alpha", serde_json::json!((v * 100.0).round() / 100.0));
            }
        }
        InvalidateRect(Some(self.hwnd), None, false);
    }

    unsafe fn set_override(&mut self, key: &str, v: serde_json::Value) {
        if self.overrides.is_null() {
            self.overrides = serde_json::json!({});
        }
        if let Some(o) = self.overrides.as_object_mut() {
            o.insert(key.to_string(), v);
        }
        self.themes.all[self.themes.active].apply_json(&self.overrides);
        self.theme_rev += 1;
        self.gfx.bg = None;
        self.gfx.bg_key = (0, 0, 0);
    }

    fn save_theme(&self) {
        self.themes.save(&self.overrides);
    }

    unsafe fn click(&mut self, h: Hot) {
        match h {
            Hot::Min => {
                ShowWindow(self.hwnd, SW_MINIMIZE);
            }
            Hot::Max => {
                if IsZoomed(self.hwnd).as_bool() {
                    ShowWindow(self.hwnd, SW_RESTORE);
                } else {
                    ShowWindow(self.hwnd, SW_MAXIMIZE);
                }
            }
            Hot::Close => {
                PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).ok();
            }
            Hot::Theme => {
                self.settings = !self.settings;
            }
            Hot::PanelClose => {
                self.settings = false;
            }
            Hot::Font(i) => {
                let fams: [&[&str]; 3] = [
                    &["Segoe UI Variable Text", "Microsoft YaHei UI", "Segoe UI", "Segoe UI Variable Display"],
                    &["Microsoft YaHei UI", "Segoe UI Variable Text", "Segoe UI", "DengXian"],
                    &["Cascadia Mono", "Consolas", "Cascadia Code"],
                ];
                let ai = self.themes.active;
                let cur = match i {
                    0 => self.themes.all[ai].font_ui.clone(),
                    1 => self.themes.all[ai].font_text.clone(),
                    _ => self.themes.all[ai].font_mono.clone(),
                };
                if let Some(list) = fams.get(i as usize) {
                    let pos = list.iter().position(|f| *f == cur.as_str()).unwrap_or(list.len() - 1);
                    let next = list[(pos + 1) % list.len()];
                    let key = match i {
                        0 => "font_ui",
                        1 => "font_text",
                        _ => "font_mono",
                    };
                    self.set_override(key, serde_json::json!(next));
                    self.save_theme();
                }
            }
            Hot::Toc => {
                self.toc_open = !self.toc_open;
            }
            Hot::Preset(i) => {
                if i < self.themes.all.len() {
                    self.overrides = serde_json::Value::Null;
                    self.themes.active = i;
                    self.theme_rev += 1;
                    self.gfx.bg = None;
                    self.gfx.bg_key = (0, 0, 0);
                    self.set_theme_dark_mode();
                    self.themes.save(&self.overrides);
                }
            }
            Hot::Recent(i) => {
                if let Some(p) = self.recent.get(i).cloned() {
                    let pb = PathBuf::from(&p);
                    if pb.exists() {
                        self.open_file(&pb);
                    } else {
                        self.recent = theme::load_recent();
                        self.status = Some(("这个文件已经不在了".to_string(), 2.0));
                        InvalidateRect(Some(self.hwnd), None, false);
                    }
                }
            }
            Hot::OpenThemes => {                let dir = theme::themes_dir();
                let _ = std::fs::create_dir_all(&dir);
                let s = HSTRING::from(dir.to_string_lossy().to_string());
                ShellExecuteW(None, w!("open"), &s, None, None, SW_SHOWNORMAL);
            }
            Hot::ResetTheme => {
                self.overrides = serde_json::Value::Null;
                let presets = theme::presets();
                let name = self.th().name.clone();
                if let Some(i) = presets.iter().position(|p| p.name == name) {
                    self.themes.all[self.themes.active] = presets[i].clone();
                }
                self.theme_rev += 1;
                self.gfx.bg = None;
                self.themes.save(&self.overrides);
            }
            Hot::Swatch(i) => {
                let sw: [&str; 6] = ["#0E8FB8", "#5851D6", "#E05A8C", "#2AA97B", "#C98A24", "#D4574E"];
                if let Some(hx) = sw.get(i) {
                    if let Some(c) = theme::parse_hex(hx) {
                        let a2 = mix(c, [1.0, 1.0, 1.0, 1.0], 0.30);
                        self.set_override("accent", serde_json::json!(hx));
                        self.set_override("accent2", serde_json::json!(theme::to_hex(a2)));
                        self.save_theme();
                    }
                }
            }
            Hot::TocItem(i) => {
                let y = self.doc.as_ref().and_then(|d| d.toc.get(i).map(|t| t.y)).unwrap_or(0.0);
                self.scroll_to(y);
            }
            Hot::CopyCode(i) => {                if let Some(txt) = self.doc.as_ref().and_then(|d| d.code_text(i)) {
                    self.copy_text(&txt);
                    self.status = Some(("已复制".into(), 1.6));
                    SetTimer(Some(self.hwnd), 1, 8, None);
                }
            }
            _ => {}
        }
        InvalidateRect(Some(self.hwnd), None, false);
    }

    unsafe fn key(&mut self, vk: u16) {
        let ctrl = (GetKeyState(0x11) as u32 & 0x8000) != 0;
        match vk {
            0x1B => {
                if self.settings {
                    self.settings = false;
                } else if IsZoomed(self.hwnd).as_bool() {
                    ShowWindow(self.hwnd, SW_RESTORE);
                } else {
                    PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).ok();
                }
            }
            0x54 => {
                if !ctrl {
                    self.toc_open = !self.toc_open;
                    InvalidateRect(Some(self.hwnd), None, false);
                }
            }
            0x4F => {
                if ctrl {
                    self.open_dialog();
                    self.doc = None;
                    InvalidateRect(Some(self.hwnd), None, false);
                }
            }
            0x45 => {
                if ctrl {
                    self.settings = !self.settings;
                }
            }
            0xBF | 0x6B => {
                // Ctrl+= / Ctrl++
                if ctrl {
                    self.zoom = (self.zoom + 0.1).min(2.2);
                    self.invalidate_layout();
                }
            }
            0xBD | 0x6D => {
                if ctrl {
                    self.zoom = (self.zoom - 0.1).max(0.6);
                    self.invalidate_layout();
                }
            }
            0x30 => {
                if ctrl {
                    self.zoom = 1.0;
                    self.invalidate_layout();
                }
            }
            _ => {}
        }
    }

    fn invalidate_layout(&mut self) {
        if let Some(d) = self.doc.as_mut() {
            d.key = (0, 0, 0, 0);
            let _ = d;
        }
        unsafe {
            InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    unsafe fn copy_text(&self, txt: &str) {
        if OpenClipboard(Some(self.hwnd)).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        let u = text_w(txt);
        let bytes = (u.len() + 1) * 2;
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(u.as_ptr(), p, u.len());
                *p.add(u.len()) = 0;
                let _ = GlobalUnlock(h);
                let _ = SetClipboardData(13, Some(HANDLE(h.0)));
            }
        }
        let _ = CloseClipboard();
    }
}













