#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case, non_upper_case_globals)]

mod doc;
mod lang;
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
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::ProcessStatus::*;
use windows::Win32::System::Threading::*;
use windows::Win32::System::Registry::*;
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
        lang::load();

        let instance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => return,
        };
        let class = w!("SchedaWnd");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
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
        // 隐藏开关：scheda.exe --register-md，只登记关联，不开文件
        let register_only = std::env::args().any(|a| a == "--register-md");
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(app) as isize);
        if register_only {
            if let Some(a) = app_mut(hwnd) {
                a.register_md();
            }
        }

        // 窗口先藏着：DPI 拿到手、尺寸算好、摆好位置，才第一次显示。
        // 这样开窗只出现一次，不会"小 → 大 → 再大"地闪。
        apply_icon(hwnd);
        let sc = dpi_scale(hwnd);
        if let Some(a) = app_mut(hwnd) {
            if (a.scale - sc).abs() > 0.01 {
                a.scale = sc;
                a.gfx.fmts.clear();
                a.gfx.doc_fmts.clear();
                a.gfx.ascents.clear();
            }
        }
        let (mut dw, mut dh) = ((1060.0 * sc) as i32, (760.0 * sc) as i32);
        if let Some((sw, sh)) = theme::load_win_size() {
            // 存的是物理像素；比例咋变都按物理像素还原，不再乘 scale
            if sw > (600.0 * sc) as i32 && sh > (400.0 * sc) as i32 {
                dw = sw;
                dh = sh;
            }
        }
        let (rw, rh) = size_window(hwnd, dw, dh);
        if let Some(a) = app_mut(hwnd) {
            a.w = rw;
            a.h = rh;
            a.chrome();
        }
        let _ = ShowWindow(hwnd, SW_SHOW);
        // 明确不做置顶：别人家的窗口可以盖上来，任务栏也正常
        let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
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

/// 窗口尺寸统一走这里：按目标大小来，但夹进当前显示器工作区（留 8px 边），再摆个舒服的位置。
/// 不再"先创建再放大"，所以窗口不会一出来抖两下、也不会长得比屏幕还高。
unsafe fn dpi_scale(hwnd: HWND) -> f32 {
    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
    if !mon.is_invalid() {
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 {
            return dx as f32 / 96.0;
        }
    }
    let d = GetDpiForWindow(hwnd);
    if d == 0 { 1.0 } else { d as f32 / 96.0 }
}

unsafe fn size_window(hwnd: HWND, want_w: i32, want_h: i32) -> (i32, i32) {
    let (mut wl, mut wt, mut ww, mut wh) = (0i32, 0i32, 1920i32, 1080i32);
    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    if !mon.is_invalid() {
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            wl = mi.rcWork.left;
            wt = mi.rcWork.top;
            ww = (mi.rcWork.right - mi.rcWork.left).max(400);
            wh = (mi.rcWork.bottom - mi.rcWork.top).max(300);
        }
    }
    let s = dpi_scale(hwnd).max(1.0);
    let min_w = (720.0 * s) as i32;
    let min_h = (460.0 * s) as i32;
    let w = want_w.clamp(min_w.min(ww - 16), ww - 16);
    let h = want_h.clamp(min_h.min(wh - 16), wh - 16);
    let x = wl + ((ww - w) / 2).max(0);
    let y = wt + ((wh - h) / 3).max(0);
    let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    (w, h)
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

fn center_window(_hwnd: HWND) {}

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
    /// 背景图（WIC 解码，一张，路径换了才重解）
    bg_img: Option<ID2D1Bitmap>,
    bg_img_path: String,
    /// 磨砂玻璃用的背景模糊图（整窗铺满，尺寸变了/换背景才重做）
    frost: Option<ID2D1Bitmap>,
    frost_key: (i32, i32, u64, u8),
    /// 模糊图相对屏幕的缩放：模糊图 1 像素 = 屏幕 1/frost_k
    frost_k: f32,
    layer: Option<ID2D1Layer>,
}

struct BgBrushes {
    base: ID2D1LinearGradientBrush,
    glow_a: ID2D1RadialGradientBrush,
    glow_b: ID2D1RadialGradientBrush,
}

/// 主题预设名的英文标注（中文界面原样显示）
fn preset_label(name: &str) -> &str {
    if !crate::lang::is_en() {
        return name;
    }
    match name {
        "极光" => "Aurora",
        "雾凇" => "Rime",
        "墨玉" => "Ink Jade",
        "霓虹" => "Neon",
        "纸白" => "Paper",
        "晨雾" => "Dawn",
        "我的主题" => "Mine",
        _ => name,
    }
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
            bg_img: None,
            bg_img_path: String::new(),
            frost: None,
            frost_key: (0, 0, 0, 0),
            frost_k: 1.0,
            layer: None,
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
                    self.layer = rt.CreateLayer(None).ok();
                    self.frost = None;
                    self.frost_key = (0, 0, 0, 0);
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
                    self.bg_img = None;
                    self.bg_img_path.clear();
                    self.frost = None;
                    self.frost_key = (0, 0, 0, 0);
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
        self.frost = None;
        self.frost_key = (0, 0, 0, 0);
    }

    /// 把整窗背景做一张低分辨率 + 多级降采样的模糊图，磨砂面板直接抠这块来画。
    /// k：模糊图 1px = 屏幕 1/k。三级 1/3 → 1/6 → 1/12，等价于大半径高斯。
    unsafe fn build_frost(
        &mut self,
        rt: &ID2D1RenderTarget,
        w: f32,
        h: f32,
        key: &str,
        top: Color,
        bottom: Color,
        veil: f32,
        dark: bool,
    ) {
        let hk = format!("{}|{:.0}|{:.0}|{}|{}", key, w, h, dark as u8, veil.to_bits() as u64);
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hh = std::collections::hash_map::DefaultHasher::new();
            hk.hash(&mut hh);
            hh.finish()
        };
        let kk = (w as i32, h as i32, hash, dark as u8);
        if self.frost.is_some() && self.frost_key == kk {
            return;
        }
        self.frost = None;
        self.frost_key = kk;

        let px_fmt = D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED };
        let mut cur: Option<ID2D1Bitmap> = None;
        // 第一级：1/2 屏幕，直接从壁纸缩下来；再 1/4、1/8 —— 停在 1/8，
        // 既糊得均匀，又留得住大片色块（1/12 会把颜色全糊平，玻璃就没味道了）
        let mut k = 0.5f32;
        for level in 0..3 {
            let bw = ((w * k).round() as u32).max(2);
            let bh = ((h * k).round() as u32).max(2);
            let size = D2D_SIZE_U { width: bw, height: bh };
            let brt = match rt.CreateCompatibleRenderTarget(None, Some(&size), Some(&px_fmt), D2D1_COMPATIBLE_RENDER_TARGET_OPTIONS_NONE) {
                Ok(t) => t,
                Err(_) => return,
            };
            brt.BeginDraw();
            // 底：主题渐变铺满
            let bwf = bw as f32;
            let bhf = bh as f32;
            if let Ok(g) = linear(&brt, &[(0.0, top), (1.0, bottom)], (0.0, 0.0), (bwf * 0.3, bhf)) {
                brt.FillRectangle(&rectf(0.0, 0.0, bwf, bhf), &g);
            }
            if let Some(prev) = cur.as_ref() {
                // 上一级整张缩进来：线性采样本身就是一次平滑，三级叠加够糊
                let _ = brt.DrawBitmap(prev, Some(&rectf(0.0, 0.0, bwf, bhf)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, Some(&rectf(0.0, 0.0, prev.GetSize().width, prev.GetSize().height)));
            } else if !key.is_empty() {
                if let Some(img) = self.bg_bitmap(rt, key) {
                    let sz = img.GetSize();
                    if sz.width > 1.0 && sz.height > 1.0 {
                        let cov = (bwf / sz.width).max(bhf / sz.height);
                        let dw = sz.width * cov;
                        let dh = sz.height * cov;
                        let dest = rectf((bwf - dw) * 0.5, (bhf - dh) * 0.5, dw, dh);
                        let _ = brt.DrawBitmap(&img, Some(&dest), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, Some(&rectf(0.0, 0.0, sz.width, sz.height)));
                    }
                }
            }
            if veil > 0.001 {
                if let Ok(bb) = brt.CreateSolidColorBrush(&cf(alpha(top, veil)), None) {
                    brt.FillRectangle(&rectf(0.0, 0.0, bwf, bhf), &bb);
                }
            }
            let _ = brt.EndDraw(None, None);
            match brt.GetBitmap() {
                Ok(b) => cur = Some(b),
                Err(_) => return,
            }
            k *= 0.5;
        }
        self.frost = cur;
        self.frost_k = 1.0 / 8.0;
    }

    /// 背景图：WIC 解码一次，key 不变就一直复用这张位图。
    /// key = "@builtin" 时用编进 exe 的默认壁纸，否则当文件路径。
    unsafe fn bg_bitmap(&mut self, rt: &ID2D1RenderTarget, key: &str) -> Option<ID2D1Bitmap> {
        if self.bg_img_path == key {
            if let Some(b) = self.bg_img.as_ref() {
                return Some(b.clone());
            }
        }
        self.bg_img = None;
        self.bg_img_path = key.to_string();
        if key.is_empty() {
            return None;
        }
        let f: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let conv = f.CreateFormatConverter().ok()?;
        if key == "@builtin" {
            let bytes: &'static [u8] = include_bytes!("../assets/背景.jpg");
            let stream = f.CreateStream().ok()?;
            stream.InitializeFromMemory(bytes).ok()?;
            let dec = f.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad).ok()?;
            let frame = dec.GetFrame(0).ok()?;
            conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)
                .ok()?;
        } else {
            if !std::path::Path::new(key).exists() {
                return None;
            }
            let dec = f
                .CreateDecoderFromFilename(&HSTRING::from(key), None, GENERIC_READ, WICDecodeMetadataCacheOnLoad)
                .ok()?;
            let frame = dec.GetFrame(0).ok()?;
            conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut)
                .ok()?;
        }
        let bmp = rt.CreateBitmapFromWicBitmap(&conv, None).ok();
        self.bg_img = bmp.clone();
        bmp
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
    Mode(bool), // true = 浅色
    BgToggle,
    BgPick,
    Assoc,
    SbJump,
    SbDrag,
    TocSb,
    TocJump,
    TocDrag,
    Lock,
    Lang(u8),
    ImageHit(usize),
    ViewerClose,
    ViewerFit,
    ViewerOne,
    ViewerCopy,
    ViewerImg,
    EditText,
    EditSbJump,
    EditSbDrag,
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
    mode_rects: [R; 2],
    bg_toggle: R,
    bg_pick: R,
    assoc_btn: R,
    // 正文滚动条（拖动）
    sb_track: R,
    sb_thumb: R,
    sb_drag: bool,
    sb_grab: f32,
    // 目录面板（可上下滚）
    toc_panel: R,
    toc_track: R,
    toc_thumb: R,
    toc_scroll: f32,
    toc_scroll_t: f32,
    toc_max: f32,
    toc_drag: bool,
    toc_grab: f32,
    toc_rect: R,
    /// 自己拖标题栏：抓点 + 起拖时的窗口左上角（不依赖系统 HTCAPTION，永远拖得动）
    win_drag: Option<(f32, f32, i32, i32)>,
    /// 背景：default / none / 自定义图片路径
    bg: String,
    recent: Vec<String>,
    recent_rects: Vec<R>,
    /// 欢迎页「最近打开」卡片：滚动位置 / 上限 / 命中矩形
    wrec_scroll: f32,
    wrec_max: f32,
    wrec_rect: R,

    // —— 只读 / 编辑
    editing: bool,
    edit_buf: String,
    edit_caret: usize,
    edit_scroll: f32,
    edit_scroll_t: f32,
    edit_layout: Option<IDWriteTextLayout>,
    edit_key: (u32, u32, u32),
    edit_dirty: bool,
    edit_caret_px: (f32, f32, f32),
    /// 上次离开编辑时光标在哪、滚到哪 —— 再点开锁还回到原处
    edit_mem_caret: usize,
    edit_mem_scroll: f32,
    edit_mem_scroll_t: f32,
    /// 上下键想去的列（避免按上下键时列号一路漂）
    edit_goal_col: Option<usize>,
    edit_track: R,
    edit_thumb: R,
    edit_drag: bool,
    edit_grab: f32,
    btn_lock: R,
    lang_pills: [R; 2],

    // —— 图片查看器
    viewer: Option<Viewer>,
    viewer_btn_rects: [R; 4],
    viewer_img_rect: R,

    status: Option<(String, f32)>,
}

/// 图片查看器的状态：一张图 + 缩放 + 平移
struct Viewer {
    bmp: Option<ID2D1Bitmap>,
    path: PathBuf,
    zoom: f32,
    ox: f32,
    oy: f32,
    drag: bool,
    grab: (f32, f32),
    /// 适应窗口后的基准缩放
    fit: f32,
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
        themes.rebuild();

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
            mode_rects: [R::default(); 2],
            bg_toggle: R::default(),
            bg_pick: R::default(),
            assoc_btn: R::default(),
            sb_track: R::default(),
            sb_thumb: R::default(),
            sb_drag: false,
            sb_grab: 0.0,
            toc_panel: R::default(),
            toc_track: R::default(),
            toc_thumb: R::default(),
            toc_scroll: 0.0,
            toc_scroll_t: 0.0,
            toc_max: 0.0,
            toc_drag: false,
            toc_grab: 0.0,
            toc_rect: R::default(),
            win_drag: None,
            bg: theme::load_bg(),
            recent: theme::load_recent(),
            recent_rects: Vec::new(),
            wrec_scroll: 0.0,
            wrec_max: 0.0,
            wrec_rect: R::default(),
            editing: false,
            edit_buf: String::new(),
            edit_caret: 0,
            edit_scroll: 0.0,
            edit_scroll_t: 0.0,
            edit_layout: None,
            edit_key: (0, 0, 0),
            edit_dirty: false,
            edit_caret_px: (0.0, 0.0, 0.0),
            edit_track: R::default(),
            edit_thumb: R::default(),
            edit_drag: false,
            edit_grab: 0.0,
            edit_mem_caret: 0,
            edit_mem_scroll: 0.0,
            edit_mem_scroll_t: 0.0,
            edit_goal_col: None,
            btn_lock: R::default(),
            lang_pills: [R::default(); 2],
            viewer: None,
            viewer_btn_rects: [R::default(); 4],
            viewer_img_rect: R::default(),
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
        self.themes.cur()
    }

    /// 背景取值 → 解码 key：内置壁纸用 "@builtin"，关掉是空串，其余是文件路径。
    fn bg_key(&self) -> String {
        match self.bg.trim() {
            "" | "default" => "@builtin".to_string(),
            "none" => String::new(),
            p => p.to_string(),
        }
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
            self.editing = false;
            self.edit_layout = None;
            self.viewer = None;
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
            self.status = Some((crate::lang::t("打不开这个文件").to_string(), 2.0));
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
            None => crate::lang::t("Markdown 查看器").into(),
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
        let tw = if crate::lang::is_en() { 92.0 * s } else { 74.0 * s };
        self.btn_theme = R { x: after - tw, y: cy, w: tw, h: bh };
        // 锁：只读 / 编辑，紧贴「主题」左边
        let lw = 34.0 * s;
        self.btn_lock = if self.file.is_some() {
            R { x: self.btn_theme.x - 8.0 * s - lw, y: cy, w: lw, h: bh }
        } else {
            R::default()
        };
        // 目录按钮：窗口够宽才出现（否则目录面板本身也放不下）
        let toc_ok = (self.w as f32) >= 860.0 * s && self.file.is_some();
        let toc_anchor = if self.btn_lock.w > 1.0 { self.btn_lock.x } else { self.btn_theme.x };
        self.btn_toc = if toc_ok { R { x: toc_anchor - 44.0 * s, y: cy, w: 36.0 * s, h: bh } } else { R::default() };
        if self.btn_theme.x < 210.0 * s {
            self.btn_toc = R::default();
            self.btn_lock = R::default();
            self.btn_theme = R::default();
        }
    }

    unsafe fn hit(&self, x: f32, y: f32) -> Hot {
        let s = self.scale;
        // 图片查看器：盖在最上层，除了它自己的按钮，其余都算拖动/背景
        if let Some(_v) = self.viewer.as_ref() {
            for (i, r) in self.viewer_btn_rects.iter().enumerate() {
                if r.w > 1.0 && r.hit(x, y) {
                    return match i {
                        0 => Hot::ViewerFit,
                        1 => Hot::ViewerOne,
                        2 => Hot::ViewerCopy,
                        _ => Hot::ViewerClose,
                    };
                }
            }
            if self.viewer_img_rect.w > 1.0 && self.viewer_img_rect.hit(x, y) {
                return Hot::ViewerImg;
            }
            return Hot::ViewerImg;
        }
        // 设置页：整页命中区
        if self.settings && self.panel_rect.hit(x, y) {
            for (i, r) in self.lang_pills.iter().enumerate() {
                if r.w > 1.0 && r.hit(x, y) {
                    return Hot::Lang(i as u8);
                }
            }
            for (i, r) in self.mode_rects.iter().enumerate() {
                if r.w > 1.0 && r.hit(x, y) {
                    return Hot::Mode(i == 0);
                }
            }
            if self.bg_toggle.hit(x, y) {
                return Hot::BgToggle;
            }
            if self.bg_pick.hit(x, y) {
                return Hot::BgPick;
            }
            if self.assoc_btn.hit(x, y) {
                return Hot::Assoc;
            }
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
        if self.btn_lock.w > 1.0 && self.btn_lock.hit(x, y) {
            return Hot::Lock;
        }
        if self.btn_toc.hit(x, y) {
            return Hot::Toc;
        }
        // 编辑态：源码区、它自己的滚动条
        if self.editing {
            if self.edit_thumb.w > 1.0 && self.edit_thumb.hit(x, y) {
                return Hot::EditSbDrag;
            }
            if self.edit_track.h > 1.0 && self.edit_track.hit(x, y) {
                return Hot::EditSbJump;
            }
            let (_, text) = self.edit_geo();
            if text.hit(x, y) {
                return Hot::EditText;
            }
            if y < TITLE_H * s {
                return Hot::None;
            }
            return Hot::Content;
        }
        if self.toc_thumb.w > 1.0 && self.toc_thumb.hit(x, y) {
            return Hot::TocDrag;
        }
        if self.toc_track.h > 1.0 && self.toc_track.hit(x, y) {
            return Hot::TocJump;
        }
        for (i, r) in self.toc_entries.iter().enumerate() {
            if r.hit(x, y) {
                return Hot::TocItem(i);
            }
        }
        if self.sb_thumb.w > 1.0 && self.sb_thumb.hit(x, y) {
            return Hot::SbDrag;
        }
        if self.sb_track.h > 1.0 && self.sb_track.hit(x, y) {
            return Hot::SbJump;
        }
        for (i, r) in self.recent_rects.iter().enumerate() {
            if r.hit(x, y) {
                return Hot::Recent(i);
            }
        }
        if let Some(d) = self.doc.as_ref() {
            if d.image_hit(x, y).is_some() {
                return Hot::ImageHit(0);
            }
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
        let title: Vec<u16> = crate::lang::t("打开 Markdown").encode_utf16().collect();
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

    unsafe fn pick_bg(&mut self) {
        let mut buf = vec![0u16; 2048];
        let filter: Vec<u16> = "图片\0*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.tif;*.tiff\0所有文件\0*.*\0"
            .encode_utf16()
            .collect();
        let title: Vec<u16> = crate::lang::t("挑一张背景图").encode_utf16().collect();
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
            let p = String::from_utf16_lossy(&buf[..end]);
            if std::path::Path::new(&p).exists() {
                self.bg = p;
                theme::set_setting("bg", serde_json::json!(self.bg.clone()));
                self.gfx.bg_img = None;
                self.gfx.bg_img_path.clear();
                self.status = Some((crate::lang::t("背景已换").to_string(), 1.6));
                SetTimer(Some(self.hwnd), 1, 8, None);
            }
        }
    }

    /// 注册 .md 的 ProgID（HKCU，随时可撤销）：图标不再是那个 exe 壳子，右键「打开方式」里能看到 Scheda。
    unsafe fn register_md(&mut self) {
        let exe = std::env::current_exe().unwrap_or_default();
        let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let ico = {
            let a = dir.join("md.ico");
            if a.exists() { a } else { dir.join("assets").join("md.ico") }
        };
        let icon = if ico.exists() { format!("{},0", ico.to_string_lossy()) } else { format!("{},0", exe.to_string_lossy()) };
        let cmd = format!("\"{}\" \"%1\"", exe.to_string_lossy());
        let ok = reg_set("Software\\Classes\\Scheda.md", None, Some("Scheda Markdown 文档"))
            && reg_set("Software\\Classes\\Scheda.md\\DefaultIcon", None, Some(&icon))
            && reg_set("Software\\Classes\\Scheda.md\\shell\\open\\command", None, Some(&cmd))
            && reg_set("Software\\Classes\\.md\\OpenWithProgids", Some("Scheda.md"), None)
            // 「打开方式」里选的是 Applications\scheda.exe，这个 ProgID 也得挂上文档图标
            && reg_set("Software\\Classes\\Applications\\scheda.exe\\DefaultIcon", None, Some(&icon))
            && reg_set("Software\\Classes\\Applications\\scheda.exe\\shell\\open\\command", None, Some(&cmd))
            && reg_set("Software\\Classes\\Applications\\scheda.exe\\SupportedTypes", Some(".md"), None);
        if ok {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
            self.status = Some((crate::lang::t("已注册 · 资源管理器里选「Scheda」设为默认").to_string(), 3.0));
        } else {
            self.status = Some((crate::lang::t("注册没写成，可能被系统拦了").to_string(), 3.0));
        }
        SetTimer(Some(self.hwnd), 1, 8, None);
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
                // 八方向缩放：边 8pt、角 14pt（角要比边宽，斜着拖才抓得住）
                let b = 8.0 * s;
                let c = 14.0 * s;
                let (nl, nr) = (x < c, x > w - c);
                let (nt, nb) = (y < c, y > h - c);
                let (el, er) = (x < b, x > w - b);
                let (et, eb) = (y < b, y > h - b);
                let ht = if (nl || nr) && (nt || nb) {
                    if nt {
                        if nl { HTTOPLEFT } else { HTTOPRIGHT }
                    } else if nl {
                        HTBOTTOMLEFT
                    } else {
                        HTBOTTOMRIGHT
                    }
                } else if el {
                    HTLEFT
                } else if er {
                    HTRIGHT
                } else if et {
                    HTTOP
                } else if eb {
                    HTBOTTOM
                } else {
                    0
                };
                if ht != 0 {
                    return LRESULT(ht as isize);
                }
            }
            // 标题栏自己拖（在 WM_LBUTTONDOWN 里搬窗口），不再交给系统的 HTCAPTION 移动循环。
            // 这条路径只依赖我们自己的消息，任何状态下都拖得动；也不会因为窗口比屏幕高而卡住。
            LRESULT(HTCLIENT as isize)
        }
        WM_GETMINMAXINFO => {
            let s = app_mut(hwnd).map(|a| a.scale).unwrap_or(1.0).max(1.0);
            let mmi = lparam.0 as *mut MINMAXINFO;
            if !mmi.is_null() {
                (*mmi).ptMinTrackSize.x = (700.0 * s) as i32;
                (*mmi).ptMinTrackSize.y = (460.0 * s) as i32;
                // 上限就是工作区：窗口再也不会被拉得比屏幕还高、还宽，然后拖不动也关不掉
                let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                if !mon.is_invalid() {
                    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                    if GetMonitorInfoW(mon, &mut mi).as_bool() {
                        let ww = (mi.rcWork.right - mi.rcWork.left).max(320);
                        let wh = (mi.rcWork.bottom - mi.rcWork.top).max(240);
                        (*mmi).ptMaxSize.x = ww;
                        (*mmi).ptMaxSize.y = wh;
                        (*mmi).ptMaxPosition.x = mi.rcWork.left;
                        (*mmi).ptMaxPosition.y = mi.rcWork.top;
                        (*mmi).ptMaxTrackSize.x = ww;
                        (*mmi).ptMaxTrackSize.y = wh;
                    }
                }
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
                if std::env::var_os("SCHEDA_TRACE").is_some() {
                    eprintln!("SIZE w={} h={} mode={}", app.w, app.h, wparam.0);
                }
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
                let new = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
                app.scale = new;
                app.gfx.fmts.clear();
                app.gfx.doc_fmts.clear();
                app.gfx.ascents.clear();
                app.gfx.bg = None;
                app.gfx.bg_key = (0, 0, 0);
                app.gfx.frost = None;
                app.gfx.frost_key = (0, 0, 0, 0);
                // 换显示器/换 DPI 时【绝不动窗口尺寸】：系统给的建议矩形会按新 DPI 再放大一遍，
                // 窗口就会莫名其妙拉长、拉宽。这里只换绘制比例，物理大小保持原样。
                app.chrome();
                InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            if let Some(app) = app_mut(hwnd) {
                theme::set_setting("win_w", serde_json::json!(app.w));
                theme::set_setting("win_h", serde_json::json!(app.h));
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
                if let Some((gx, gy, l, t)) = app.win_drag {
                    let mut nx = l + (x - gx) as i32;
                    let mut ny = t + (y - gy) as i32;
                    // 别让标题栏跑出屏幕：至少留 80pt 抓得回来
                    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
                    if !mon.is_invalid() {
                        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                        if GetMonitorInfoW(mon, &mut mi).as_bool() {
                            let wa = mi.rcWork;
                            let keep = (80.0 * app.scale) as i32;
                            nx = nx.clamp(wa.left - (app.w as i32 - keep), wa.right - keep);
                            ny = ny.clamp(wa.top, wa.bottom - 40);
                        }
                    }
                    let _ = SetWindowPos(hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    return LRESULT(0);
                }
                if let Some(v) = app.viewer.as_mut() {
                    if v.drag {
                        v.ox += x - v.grab.0;
                        v.oy += y - v.grab.1;
                        v.grab = (x, y);
                        InvalidateRect(Some(hwnd), None, false);
                    }
                }
                if app.edit_drag {
                    app.edit_drag_to(y);
                    InvalidateRect(Some(hwnd), None, false);
                } else if app.dragging_slider > 0 {
                    app.drag_slider(x);
                } else if app.sb_drag || app.toc_drag {
                    app.drag_scrollbar(y);
                    InvalidateRect(Some(hwnd), None, false);
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
                app.mouse = (x, y);
                // 标题栏空白处：自己搬窗口（不靠系统的标题栏拖动，任何情况都拖得动）
                let mut drag_start = false;
                if h == Hot::None && y <= TITLE_H * app.scale {
                    let mut wr = RECT::default();
                    if GetWindowRect(hwnd, &mut wr).is_ok() {
                        app.win_drag = Some((x, y, wr.left, wr.top));
                        drag_start = true;
                    }
                }
                if std::env::var_os("SCHEDA_TRACE").is_some() {
                    eprintln!(
                        "DOWN x={x} y={y} h={h:?} lock={:?} theme={:?} file={}",
                        app.btn_lock, app.btn_theme, app.file.is_some()
                    );
                }
                match h {
                    Hot::Slider(i) => {
                        app.dragging_slider = i + 1;
                        app.drag_slider(x);
                    }
                    Hot::SbDrag => {
                        app.sb_drag = true;
                        app.sb_grab = y - app.sb_thumb.y;
                    }
                    Hot::SbJump => {
                        app.grab_scrollbar(y);
                    }
                    Hot::TocDrag => {
                        app.toc_drag = true;
                        app.toc_grab = y - app.toc_thumb.y;
                    }
                    Hot::TocJump => {
                        app.grab_toc(y);
                    }
                    Hot::ViewerImg => {
                        if let Some(v) = app.viewer.as_mut() {
                            v.drag = true;
                            v.grab = (x, y);
                        }
                    }
                    Hot::EditSbDrag => {
                        app.edit_drag = true;
                        app.edit_grab = y - app.edit_thumb.y;
                    }
                    Hot::EditSbJump => {
                        let maxs = app.edit_max_scroll();
                        let span = (app.edit_track.h - app.edit_thumb.h).max(1.0);
                        let t = ((y - app.edit_track.y - app.edit_thumb.h * 0.5) / span).clamp(0.0, 1.0);
                        app.edit_scroll = t * maxs;
                        app.edit_scroll_t = app.edit_scroll;
                    }
                    Hot::EditText => {
                        app.edit_click(x, y);
                    }
                    _ => {}
                }
                // 只在拖拽/滑杆时抓鼠标；普通点击不抓，别的窗口才盖得上来
                if matches!(
                    h,
                    Hot::Slider(_) | Hot::SbDrag | Hot::SbJump | Hot::TocDrag | Hot::TocJump | Hot::ViewerImg | Hot::EditSbDrag | Hot::EditSbJump
                ) || drag_start
                {
                    let _ = SetCapture(hwnd);
                }
                InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            if let Some(app) = app_mut(hwnd) {
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                let h = app.hit(x, y);
                if h == Hot::None && y <= TITLE_H * app.scale {
                    // 双击标题栏＝最大化 / 还原（现在标题栏是我们自己拖的，这活儿也得自己干）
                    if IsZoomed(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    } else {
                        let _ = ShowWindow(hwnd, SW_MAXIMIZE);
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(app) = app_mut(hwnd) {
                let _ = ReleaseCapture();
                let x = (lparam.0 & 0xFFFF) as i16 as f32;
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                app.edit_drag = false;
                if let Some(v) = app.viewer.as_mut() {
                    v.drag = false;
                }
                let h = app.hit(x, y);
                let pressed = app.pressed;
                let was_drag = app.dragging_slider > 0;
                let was_move = app.win_drag.is_some();
                app.win_drag = None;
                app.pressed = Hot::None;
                app.dragging_slider = 0;
                app.sb_drag = false;
                app.toc_drag = false;
                if was_drag {
                    app.save_theme();
                }
                if h == pressed && !was_move {
                    app.click(h);
                }
                InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if let Some(app) = app_mut(hwnd) {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32;
                let mut p = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut p);
                app.wheel_at(delta, p.x as f32, p.y as f32);
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
        WM_CHAR => {
            if let Some(app) = app_mut(hwnd) {
                if app.editing {
                    let c = wparam.0 as u32;
                    if c == 0x09 {
                        app.edit_insert('\t');
                    } else if c >= 0x20 {
                        if let Some(ch) = char::from_u32(c) {
                            if !ch.is_control() {
                                app.edit_insert(ch);
                            }
                        }
                    }
                    InvalidateRect(Some(hwnd), None, false);
                }
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

unsafe fn reg_set(sub: &str, name: Option<&str>, val: Option<&str>) -> bool {
    let mut h = HKEY::default();
    let sub_w = HSTRING::from(sub);
    let rc = RegCreateKeyExW(
        HKEY_CURRENT_USER,
        &sub_w,
        None,
        PCWSTR::null(),
        REG_OPTION_NON_VOLATILE,
        KEY_WRITE,
        None,
        &mut h,
        None,
    );
    if rc.is_err() {
        return false;
    }
    let name_h = name.map(HSTRING::from);
    let nm = name_h.as_ref().map(|x| PCWSTR::from_raw(x.as_ptr())).unwrap_or(PCWSTR::null());
    let ok = match val {
        Some(v) => {
            let w: Vec<u16> = v.encode_utf16().chain(std::iter::once(0)).collect();
            let mut bytes: Vec<u8> = Vec::with_capacity(w.len() * 2);
            for c in w {
                bytes.push(c as u8);
                bytes.push((c >> 8) as u8);
            }
            RegSetValueExW(h, nm, None, REG_SZ, Some(&bytes)).is_ok()
        }
        None => RegSetValueExW(h, nm, None, REG_NONE, None).is_ok(),
    };
    let _ = RegCloseKey(h);
    ok
}

unsafe fn fill_r(rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, r: R, c: Color) {
    b.SetColor(&cf(c));
    rt.FillRectangle(&rectf(r.x, r.y, r.w, r.h), b);
}

/// 中性玻璃涂层：只分深浅，不带任何色相。
/// 真磨砂玻璃是"把背后磨糊"，不是"染一层颜色"——所以这里永远只用纯白 / 纯黑，
/// 颜色全部留给底下那张磨砂图，玻璃只负责压一点亮度和厚度。
pub(crate) fn glass(dark: bool, a: f32) -> Color {
    if dark {
        [0.07, 0.07, 0.08, a]
    } else {
        [1.0, 1.0, 1.0, a]
    }
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

    /// 一层磨砂玻璃：先抠一块背景模糊图铺进圆角里，再压一层半透明着色 + 噪点 + 高光 + 发丝边 + 外投影。
    /// `tint` 的 RGB 是玻璃颜色，alpha 是这层玻璃有多crate::lang::t("厚")（越小越透，背景越明显）。
    unsafe fn frost_panel(&self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, r: R, rad: f32, tint: Color, shadow: bool) {
        let s = self.scale;
        let rad = rad.min(r.w * 0.5).min(r.h * 0.5);
        if shadow && th.shadow > 0.01 {
            // 橡胶似的软投影：大范围低透明 + 近处稍实
            for i in 0..7 {
                let grow = (i as f32 + 1.0) * 3.4 * s;
                let a = th.shadow * 0.030 * (1.0 - i as f32 / 7.0);
                let c = if th.is_dark() { [0.0, 0.0, 0.0, a] } else { [0.05, 0.09, 0.14, a * 0.85] };
                fill_round(rt, b, r.x - grow, r.y - grow + 4.0 * s, r.w + grow * 2.0, r.h + grow * 2.0, rad + grow, c);
            }
        }

        // ① 抠背景模糊图（圆角遮罩，玻璃才有crate::lang::t("后面那层被磨过")的感觉）
        let mut frosted = false;
        if let (Some(frost), Some(layer)) = (self.gfx.frost.as_ref(), self.gfx.layer.as_ref()) {
            if let Ok(g) = self.gfx.factory.CreateRoundedRectangleGeometry(&rrect(r.x, r.y, r.w, r.h, rad)) {
                if let Ok(geometry) = g.cast::<ID2D1Geometry>() {
                    let params = D2D1_LAYER_PARAMETERS {
                        contentBounds: D2D_RECT_F { left: -1.0e6, top: -1.0e6, right: 1.0e6, bottom: 1.0e6 },
                        geometricMask: std::mem::ManuallyDrop::new(Some(geometry)),
                        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                        maskTransform: ident(),
                        opacity: 1.0,
                        opacityBrush: std::mem::ManuallyDrop::new(None),
                        layerOptions: D2D1_LAYER_OPTIONS_NONE,
                    };
                    rt.PushLayer(&params, layer);
                    let fk = self.gfx.frost_k;
                    let _ = rt.DrawBitmap(
                        frost,
                        Some(&rectf(r.x, r.y, r.w, r.h)),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        Some(&rectf(r.x * fk, r.y * fk, r.w * fk, r.h * fk)),
                    );
                    rt.PopLayer();
                    frosted = true;
                }
            }
        }

        // ② 着色：一层上亮下暗的玻璃色（alpha 越小，背后的颜色越明显）
        if frosted {
            let a0 = (tint[3] * 1.04).min(0.94);
            let a1 = (tint[3] * 0.90).min(0.92);
            if let Ok(g) = linear(rt, &[(0.0, alpha(tint, a0)), (1.0, alpha(tint, a1))], (r.x, r.y), (r.x, r.y + r.h)) {
                rt.FillRoundedRectangle(&rrect(r.x, r.y, r.w, r.h, rad), &g);
            }
        } else {
            fill_round(rt, b, r.x, r.y, r.w, r.h, rad, alpha(tint, 0.96));
        }

        // ②b 玻璃厚度：贴边上沿压一道淡阴影、下沿反一点光，卡片才像"一块"而不是"一张纸"
        {
            let sh = if th.is_dark() { [0.0, 0.0, 0.0, 0.20] } else { [0.06, 0.10, 0.16, 0.10] };
            let d = 10.0 * s;
            if let Ok(g) = linear(rt, &[(0.0, sh), (1.0, alpha(sh, 0.0))], (r.x, r.y), (r.x, r.y + d)) {
                rt.FillRoundedRectangle(&rrect(r.x, r.y, r.w, r.h, rad), &g);
            }
            let bl = if th.is_dark() { [1.0, 1.0, 1.0, 0.03] } else { [1.0, 1.0, 1.0, 0.10] };
            if let Ok(g) = linear(rt, &[(0.0, alpha(bl, 0.0)), (1.0, bl)], (r.x, r.y + r.h - d * 1.6), (r.x, r.y + r.h)) {
                rt.FillRoundedRectangle(&rrect(r.x, r.y, r.w, r.h, rad), &g);
            }
        }

        // ③ 细噪点：模拟玻璃表面的粗糙
        if let Some(nb) = self.gfx.noise_brush.clone() {
            nb.SetOpacity(0.055 * th.frost);
            rt.FillRoundedRectangle(&rrect(r.x, r.y, r.w, r.h, rad), &nb);
        }
        // ④ 上边缘一道高光 + 左下角一点更亮的反光
        let hi = if th.is_dark() { [1.0, 1.0, 1.0, 0.10] } else { [1.0, 1.0, 1.0, 0.62] };
        fill_round(rt, b, r.x + rad * 0.5, r.y + 0.5 * s, (r.w - rad).max(1.0), 1.0 * s, 0.5, hi);
        if let Ok(g) = linear(rt, &[(0.0, [1.0, 1.0, 1.0, if th.is_dark() { 0.05 } else { 0.14 }]), (1.0, [1.0, 1.0, 1.0, 0.0])], (r.x, r.y + r.h * 0.55), (r.x, r.y + r.h)) {
            rt.FillRoundedRectangle(&rrect(r.x, r.y, r.w, r.h, rad), &g);
        }
        // ⑤ 发丝边
        stroke_round(
            rt,
            b,
            r.x,
            r.y,
            r.w,
            r.h,
            rad,
            alpha(if th.is_dark() { [1.0, 1.0, 1.0, 1.0] } else { [0.06, 0.11, 0.16, 1.0] }, th.border_alpha),
            1.0,
        );
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
        // 1.5) 背景图：cover 铺满 + 一层薄雾，正文才压得住
        let key = self.bg_key();
        let veil = if th.is_dark() { 0.55 } else { 0.06 };
        if !key.is_empty() {
            if let Some(img) = self.gfx.bg_bitmap(&rt, &key) {
                let sz = img.GetSize();
                if sz.width > 1.0 && sz.height > 1.0 {
                    let k = (w / sz.width).max(h / sz.height);
                    let dw = sz.width * k;
                    let dh = sz.height * k;
                    let dr = rectf((w - dw) * 0.5, (h - dh) * 0.5, dw, dh);
                    let sr = rectf(0.0, 0.0, sz.width, sz.height);
                    let _ = rt.DrawBitmap(
                        &img,
                        Some(&dr),
                        1.0,
                        D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                        Some(&sr),
                    );
                    fill_r(&rt, &b, R { x: 0.0, y: 0.0, w, h }, alpha(th.bg_top, veil));
                }
            }
        }
        if let Some(nb) = self.gfx.noise_brush.clone() {
            nb.SetOpacity(0.030 * th.frost);
            rt.FillRectangle(&rectf(0.0, 0.0, w, h), &nb);
        }
        // 1.6) 给磨砂玻璃准备一张背景模糊图（尺寸/背景/主题变了才重做）
        self.gfx.build_frost(&rt, w, h, &key, th.bg_top, th.bg_bottom, if key.is_empty() { 0.0 } else if th.is_dark() { 0.22 } else { 0.02 }, th.is_dark());

        // 2) 内容
        if self.editing {
            self.paint_edit(&rt, &b, &th, w, h);
        } else if self.settings {
            self.paint_settings(&rt, &b, &th, w, h);
        } else if self.file.is_some() {
            self.paint_doc(&rt, &b, &th, w, h);
        } else {
            self.paint_welcome(&rt, &b, &th, w, h);
        }

        // 3) 标题栏
        self.paint_titlebar(&rt, &b, &th, w);

        // 4) 图片查看器：压在所有东西上面
        if self.viewer.is_some() {
            self.paint_viewer(&rt, &b, &th, w, h);
        }

        // 5) 提示
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
        let m = 38.0 * s;

        // —— 品牌 + 卡片作为一整块，在窗口里上下居中
        let ls = 46.0 * s;
        let ch = ((h - TITLE_H * s - 26.0 * s - 58.0 * s) * 0.64).clamp(180.0 * s, 520.0 * s);
        let group_h = ls + 30.0 * s + ch;
        let mut by = TITLE_H * s + (h - TITLE_H * s - group_h) * 0.5;
        by = by.max(TITLE_H * s + 22.0 * s);
        if by + group_h > h - 46.0 * s {
            by = (h - 46.0 * s - group_h).max(TITLE_H * s + 22.0 * s);
        }
        let top = by + ls + 30.0 * s;
        if let Ok(g) = linear(rt, &[(0.0, th.accent), (1.0, th.accent2)], (m, by), (m + ls, by + ls)) {
            rt.FillRoundedRectangle(&rrect(m, by, ls, ls, 12.0 * s), &g);
        }
        fill_round(rt, b, m + 14.0 * s, by + 10.0 * s, 18.0 * s, 26.0 * s, 4.0 * s, [1.0, 1.0, 1.0, 0.94]);
        for i in 0..3 {
            fill_round(
                rt,
                b,
                m + 18.0 * s,
                by + 15.0 * s + i as f32 * 7.0 * s,
                if i < 2 { 10.0 } else { 6.0 } * s,
                2.0 * s,
                1.0 * s,
                alpha(th.accent, 0.9),
            );
        }
        self.text_ui(rt, b, "Scheda", R { x: m + ls + 18.0 * s, y: by - 3.0 * s, w: 420.0 * s, h: 36.0 * s }, 25.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        self.text_ui(
            rt,
            b,
            crate::lang::t("把 Markdown 读成它该有的样子"),
            R { x: m + ls + 18.0 * s, y: by + 30.0 * s, w: 560.0 * s, h: 20.0 * s },
            11.5 * s,
            400,
            alpha(th.muted, 0.95),
            DWRITE_TEXT_ALIGNMENT_LEADING,
        );

        // —— 两张等宽等高的玻璃卡，左右排
        let gap = 18.0 * s;
        let cw = ((w - m * 2.0 - gap) * 0.5).max(240.0);
        let lc = R { x: m, y: top, w: cw, h: ch };
        let rc = R { x: m + cw + gap, y: top, w: cw, h: ch };
        let fillc = glass(th.is_dark(), if th.is_dark() { 0.34 } else { 0.31 });
        self.frost_panel(rt, b, th, lc, th.radius, fillc, true);
        self.frost_panel(rt, b, th, rc, th.radius, fillc, true);

        // —— 左卡：拖文件进来 + 快捷键（竖排，一行一个）
        let ix = lc.x + 26.0 * s;
        let iy = lc.y + 26.0 * s;
        fill_round(rt, b, ix, iy, 36.0 * s, 44.0 * s, 9.0 * s, alpha(th.accent, 0.14));
        stroke_round(rt, b, ix, iy, 36.0 * s, 44.0 * s, 9.0 * s, alpha(th.accent, 0.45), 1.0);
        for i in 0..3 {
            fill_round(
                rt,
                b,
                ix + 9.0 * s,
                iy + 11.0 * s + i as f32 * 9.0 * s,
                if i < 2 { 18.0 } else { 11.0 } * s,
                2.2 * s,
                1.1 * s,
                alpha(th.accent, 0.85),
            );
        }
        self.text_ui(
            rt,
            b,
            crate::lang::t("把 .md 文件拖进来"),
            R { x: ix + 50.0 * s, y: iy - 2.0 * s, w: lc.w - 76.0 * s, h: 26.0 * s },
            17.0 * s,
            600,
            th.heading,
            DWRITE_TEXT_ALIGNMENT_LEADING,
        );
        self.text_ui(
            rt,
            b,
            crate::lang::t("或者按 Ctrl+O 选一个文件"),
            R { x: ix + 50.0 * s, y: iy + 23.0 * s, w: lc.w - 76.0 * s, h: 18.0 * s },
            11.0 * s,
            400,
            th.muted,
            DWRITE_TEXT_ALIGNMENT_LEADING,
        );

        let chips = [
            ("Ctrl+O", crate::lang::t("打开文件")),
            ("Ctrl+E", crate::lang::t("设置")),
            ("T", crate::lang::t("目录")),
            ("Ctrl+滚轮", crate::lang::t("缩放")),
        ];
        let mut cy2 = iy + 72.0 * s;
        for (k, label) in chips.iter() {
            if cy2 + 26.0 * s > lc.y + lc.h - 30.0 * s {
                break;
            }
            let kw = self.measure_ui(k, 11.5 * s, 600);
            let lw = self.measure_ui(label, 11.5 * s, 400);
            let pw = kw + lw + 34.0 * s;
            fill_round(
                rt,
                b,
                ix,
                cy2,
                pw,
                26.0 * s,
                13.0 * s,
                alpha(if th.is_dark() { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] }, if th.is_dark() { 0.06 } else { 0.05 }),
            );
            self.text_ui(rt, b, k, R { x: ix + 12.0 * s, y: cy2, w: kw + 4.0, h: 26.0 * s }, 11.5 * s, 600, th.accent, DWRITE_TEXT_ALIGNMENT_LEADING);
            self.text_ui(rt, b, label, R { x: ix + 16.0 * s + kw, y: cy2, w: lw + 4.0, h: 26.0 * s }, 11.5 * s, 400, th.muted, DWRITE_TEXT_ALIGNMENT_LEADING);
            cy2 += 36.0 * s;
        }
        if ch > 300.0 * s {
            self.text_ui(
            rt,
            b,
            crate::lang::t("只读打开 · 不动原文件 · 关窗即走"),
            R { x: ix, y: lc.y + lc.h - 26.0 * s, w: lc.w - 50.0 * s, h: 18.0 * s },
            10.0 * s,
            400,
            alpha(th.muted, 0.65),
            DWRITE_TEXT_ALIGNMENT_LEADING,
        );
        }

        // —— 右卡：最近打开，可上下滚
        self.recent_rects.clear();
        self.wrec_rect = rc;
        self.wrec_max = 0.0;
        let count = self.recent.len();
        self.text_ui(rt, b, crate::lang::t("最近打开"), R { x: rc.x + 22.0 * s, y: rc.y + 16.0 * s, w: 200.0 * s, h: 24.0 * s }, 13.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        if count > 0 {
            self.text_ui(
                rt,
                b,
                &format!("{count}"),
                R { x: rc.x + rc.w - 96.0 * s, y: rc.y + 19.0 * s, w: 74.0 * s, h: 18.0 * s },
                10.0 * s,
                400,
                alpha(th.muted, 0.75),
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            );
        }
        fill_r(rt, b, R { x: rc.x + 20.0 * s, y: rc.y + 46.0 * s, w: rc.w - 40.0 * s, h: 1.0 }, alpha(th.text, 0.06));

        let body = R { x: rc.x, y: rc.y + 54.0 * s, w: rc.w, h: (rc.h - 68.0 * s).max(0.0) };
        if count == 0 {
            self.text_ui(
                rt,
                b,
                crate::lang::t("还没有打开过文件"),
                R { x: body.x + 22.0 * s, y: body.y + 28.0 * s, w: body.w - 44.0 * s, h: 24.0 * s },
                11.5 * s,
                500,
                alpha(th.muted, 0.9),
                DWRITE_TEXT_ALIGNMENT_LEADING,
            );
            self.text_ui(
                rt,
                b,
                crate::lang::t("拖一个 .md 进来，或者按 Ctrl+O"),
                R { x: body.x + 22.0 * s, y: body.y + 56.0 * s, w: body.w - 44.0 * s, h: 20.0 * s },
                10.5 * s,
                400,
                alpha(th.muted, 0.7),
                DWRITE_TEXT_ALIGNMENT_LEADING,
            );
        } else {
            let row_h = 46.0 * s;
            let total = count as f32 * row_h;
            self.wrec_max = (total - body.h).max(0.0);
            self.wrec_scroll = self.wrec_scroll.clamp(0.0, self.wrec_max);
            rt.PushAxisAlignedClip(&rectf(body.x + 4.0 * s, body.y, body.w - 8.0 * s, body.h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            for i in 0..count {
                let p = self.recent[i].clone();
                let y0 = body.y - self.wrec_scroll + i as f32 * row_h;
                let r = R { x: body.x + 8.0 * s, y: y0, w: body.w - 16.0 * s, h: row_h - 2.0 * s };
                // 看不见的行也占位（保持 recent_rects 下标 == recent 下标）
                if y0 + row_h < body.y || y0 > body.y + body.h {
                    self.recent_rects.push(R::default());
                    continue;
                }
                self.recent_rects.push(r);
                if self.hot == Hot::Recent(i) {
                    fill_round(rt, b, r.x, r.y, r.w, r.h, 10.0 * s, alpha(th.accent, 0.10));
                }
                let icx = r.x + 12.0 * s;
                let icy = r.cy() - 13.0 * s;
                fill_round(rt, b, icx, icy, 26.0 * s, 26.0 * s, 7.0 * s, alpha(th.accent, 0.10));
                fill_round(rt, b, icx + 8.0 * s, icy + 5.0 * s, 10.0 * s, 15.0 * s, 2.5 * s, [1.0, 1.0, 1.0, 1.0]);
                for k in 0..3 {
                    fill_round(rt, b, icx + 10.0 * s, icy + 8.0 * s + k as f32 * 4.0 * s, if k < 2 { 6.0 } else { 4.0 } * s, 1.5 * s, 0.75 * s, alpha(th.accent, 0.8));
                }
                let pb = PathBuf::from(&p);
                let mut name = pb.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| p.clone());
                if name.chars().count() > 24 {
                    name = name.chars().take(22).collect::<String>() + "…";
                }
                let dir = pb.parent().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                self.text_ui(rt, b, &name, R { x: icx + 34.0 * s, y: r.y + 6.0 * s, w: r.w - 58.0 * s, h: 17.0 * s }, 12.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
                let mut dshort = if dir.chars().count() > 52 {
                    let head: String = dir.chars().take(14).collect();
                    let tail: String = dir.chars().skip(dir.chars().count() - 34).collect();
                    format!("{}…{}", head, tail)
                } else {
                    dir.clone()
                };
                if dshort.is_empty() {
                    dshort = crate::lang::t("本地文件").into();
                }
                self.text_ui(rt, b, &dshort, R { x: icx + 34.0 * s, y: r.y + 24.0 * s, w: r.w - 58.0 * s, h: 15.0 * s }, 9.5 * s, 400, alpha(th.muted, 0.9), DWRITE_TEXT_ALIGNMENT_LEADING);
            }
            rt.PopAxisAlignedClip();
            // 滚动条
            if self.wrec_max > 1.0 {
                let tx = rc.x + rc.w - 10.0 * s;
                fill_round(rt, b, tx, body.y + 2.0 * s, 4.0 * s, body.h - 4.0 * s, 2.0 * s, alpha(th.text, 0.05));
                let th_h = (body.h * body.h / total).max(34.0 * s).min(body.h);
                let t = (self.wrec_scroll / self.wrec_max).clamp(0.0, 1.0);
                fill_round(rt, b, tx, body.y + 2.0 * s + (body.h - 4.0 * s - th_h) * t, 4.0 * s, th_h, 2.0 * s, alpha(th.accent, 0.42));
            }
        }

        let fb = crate::lang::t("小方工作室 · 零联网 · 零常驻");
        let fbw = self.measure_ui(fb, 11.0 * s, 400) + 26.0 * s;
        let fy = h - 40.0 * s;
        fill_round(
            rt,
            b,
            w - m - fbw,
            fy - 4.0 * s,
            fbw,
            26.0 * s,
            13.0 * s,
            glass(th.is_dark(), if th.is_dark() { 0.20 } else { 0.16 }),
        );
        self.text_ui(
            rt,
            b,
            fb,
            R { x: w - m - fbw, y: fy, w: fbw, h: 22.0 * s },
            11.0 * s,
            400,
            alpha(th.muted, 0.9),
            DWRITE_TEXT_ALIGNMENT_CENTER,
        );
    }

    unsafe fn paint_titlebar(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, w: f32) {
        let s = self.scale;
        let thh = TITLE_H * s;

        // 玻璃条
        let tint = if th.is_dark() {
            theme::over([1.0, 1.0, 1.0, 1.0], th.bg_top, 0.030)
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
        self.win_btn(rt, b, th, self.btn_lock, self.hot == Hot::Lock, 5);
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
                    crate::lang::t("主题"),
                    R { x: cx - 5.0 * s, y: r.y, w: (r.w * 0.5 - 4.0 * s).max(30.0 * s), h: r.h },
                    11.5 * s,
                    500,
                    if hot { th.heading } else { alpha(th.text, 0.82) },
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                );
            }
            5 => {
                // 锁：只读时闭合（灰），编辑时锁梁掀开并染强调色。
                // 锁梁是半圆 + 两条腿（下半截被锁体压住），锁体带一个真正的钥匙孔，
                // 不再是"方块上顶个圆角矩形"那种草稿。
                let editing = self.editing;
                if editing {
                    // 编辑态：一枚斜着的铅笔，比"掀开的锁"更好认
                    let c = alpha(th.accent, 0.95);
                    rt.SetTransform(&rot_about(0.785, cx, cy));
                    fill_round(rt, b, cx - 2.4 * s, cy - 7.6 * s, 4.8 * s, 12.0 * s, 1.4 * s, c);
                    // 笔尖
                    fill_round(rt, b, cx - 2.4 * s, cy + 4.0 * s, 4.8 * s, 2.6 * s, 1.2 * s, alpha(th.accent2, 0.95));
                    // 笔杆高光
                    fill_round(rt, b, cx - 1.3 * s, cy - 6.6 * s, 1.2 * s, 9.6 * s, 0.6 * s, alpha([1.0, 1.0, 1.0, 1.0], 0.42));
                    // 笔帽
                    fill_round(rt, b, cx - 2.4 * s, cy - 7.6 * s, 4.8 * s, 1.8 * s, 0.9 * s, alpha(th.accent2, 0.75));
                    rt.SetTransform(&ident());
                    return;
                }
                let c = ic;
                stroke_round(rt, b, cx - 4.6 * s, cy - 11.0 * s, 9.2 * s, 12.0 * s, 4.6 * s, c, 1.6 * s);
                // 锁体（把锁梁下半截盖住，剩下就是 U 形）
                fill_round(rt, b, cx - 7.6 * s, cy - 1.8 * s, 15.2 * s, 10.0 * s, 3.4 * s, c);
                // 顶面一道弱高光，金属感
                fill_round(rt, b, cx - 5.6 * s, cy - 0.9 * s, 11.2 * s, 1.2 * s, 0.6 * s, alpha([1.0, 1.0, 1.0, 1.0], if editing { 0.55 } else { 0.30 }));
                if !editing {
                    // 钥匙孔
                    let kc = alpha(if th.is_dark() { [0.09, 0.10, 0.12, 1.0] } else { [1.0, 1.0, 1.0, 1.0] }, 0.92);
                    fill_round(rt, b, cx - 1.5 * s, cy + 1.5 * s, 3.0 * s, 3.0 * s, 1.5 * s, kc);
                    fill_round(rt, b, cx - 0.8 * s, cy + 3.4 * s, 1.6 * s, 2.9 * s, 0.8 * s, kc);
                }
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
        let card_fill = glass(th.is_dark(), if th.is_dark() { 0.34 } else { 0.31 });

        // 标题
        self.text_ui(rt, b, crate::lang::t("设置"), R { x: m, y: top, w: 260.0 * s, h: 34.0 * s }, 21.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        self.text_ui(
            rt,
            b,
            crate::lang::t("外观与排版 · 改动即时生效，自动写进 settings.json"),
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

        // 右上：把 .md 交给 Scheda（写 HKCU，用户自己点，随时可撤销）
        let aw = 176.0 * s;
        let ar = R { x: w - m - 32.0 * s - 10.0 * s - aw, y: top + 2.0 * s, w: aw, h: 30.0 * s };
        self.assoc_btn = R::default();
        if ar.x > m + 340.0 * s {
            self.assoc_btn = ar;
            let hots = self.hot == Hot::Assoc;
            fill_round(rt, b, ar.x, ar.y, ar.w, ar.h, 15.0 * s, if hots { alpha(th.accent, 0.14) } else { card_fill });
            stroke_round(rt, b, ar.x, ar.y, ar.w, ar.h, 15.0 * s, alpha(th.text, 0.08), 1.0);
            self.text_ui(rt, b, crate::lang::t("设为默认 .md 打开方式"), ar, 10.0 * s, 500, if hots { th.accent } else { alpha(th.text, 0.78) }, DWRITE_TEXT_ALIGNMENT_CENTER);
        }

        // 语言：中文 / English（就挂在关联按钮左边）
        self.lang_pills = [R::default(); 2];
        let lpw = 56.0 * s;
        let lph = 30.0 * s;
        let lx0 = ar.x - 12.0 * s - lpw * 2.0 - 6.0 * s;
        if self.assoc_btn.w > 1.0 && lx0 > m + 300.0 * s {
            self.text_ui(
                rt,
                b,
                crate::lang::t("语言"),
                R { x: lx0 - 52.0 * s, y: ar.y, w: 44.0 * s, h: lph },
                10.0 * s,
                400,
                alpha(th.muted, 0.9),
                DWRITE_TEXT_ALIGNMENT_TRAILING,
            );
            let cur = crate::lang::get();
            for i in 0..2u8 {
                let r = R { x: lx0 + i as f32 * (lpw + 6.0 * s), y: ar.y, w: lpw, h: lph };
                self.lang_pills[i as usize] = r;
                let on = cur == i;
                let hov = self.hot == Hot::Lang(i);
                fill_round(rt, b, r.x, r.y, r.w, r.h, 15.0 * s, if on { alpha(th.accent, 0.14) } else if hov { alpha(th.text, 0.05) } else { card_fill });
                if on {
                    stroke_round(rt, b, r.x, r.y, r.w, r.h, 15.0 * s, alpha(th.accent, 0.35), 1.0);
                }
                let lab = if i == 0 { crate::lang::t("中文") } else { "English" };
                self.text_ui(rt, b, lab, r, 10.0 * s, 500, if on { th.accent } else { alpha(th.text, 0.78) }, DWRITE_TEXT_ALIGNMENT_CENTER);
            }
        }

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
        self.text_ui(rt, b, crate::lang::t("主题"), R { x: lcx, y: lr.y + 18.0 * s, w: lcw, h: 22.0 * s }, 13.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);

        // 浅色 / 深色 切换（当前预设的两种明暗，不另开主题）
        let mpw = 48.0 * s;
        let mph = 24.0 * s;
        for (i, lab) in [crate::lang::t("浅色"), crate::lang::t("深色")].iter().enumerate() {
            let mr = R {
                x: lcx + lcw - (2.0 - i as f32) * (mpw + 6.0 * s) + 6.0 * s,
                y: lr.y + 16.0 * s,
                w: mpw,
                h: mph,
            };
            self.mode_rects[i] = mr;
            let on = if i == 0 { !self.themes.dark } else { self.themes.dark };
            let hot = self.hot == Hot::Mode(i == 0);
            if on {
                fill_round(rt, b, mr.x, mr.y, mr.w, mr.h, 12.0 * s, alpha(th.accent, 0.14));
                stroke_round(rt, b, mr.x, mr.y, mr.w, mr.h, 12.0 * s, alpha(th.accent, 0.35), 1.0);
            } else if hot {
                fill_round(rt, b, mr.x, mr.y, mr.w, mr.h, 12.0 * s, alpha(th.text, 0.05));
            }
            let col = if on { th.accent } else { alpha(th.muted, 1.0) };
            self.text_ui(rt, b, lab, mr, 10.5 * s, if on { 600 } else { 500 }, col, DWRITE_TEXT_ALIGNMENT_CENTER);
        }
        self.text_ui(rt, b, crate::lang::t("选中即换 · 可切深浅"), R { x: lcx, y: lr.y + 40.0 * s, w: lcw, h: 16.0 * s }, 9.5 * s, 400, alpha(th.muted, 0.85), DWRITE_TEXT_ALIGNMENT_LEADING);

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
                preset_label(pname),
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
            self.text_ui(rt, b, crate::lang::t("预览"), R { x: lcx, y: prr.y + 16.0 * s, w: lcw, h: 20.0 * s }, 11.5 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
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
                    crate::lang::t("零常驻 · 零联网 · 冷启动几十毫秒。"),
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
        self.text_ui(rt, b, crate::lang::t("强调色"), R { x: rcx, y: c1.y + 18.0 * s, w: rcw, h: 20.0 * s }, 12.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
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
        self.text_ui(rt, b, crate::lang::t("形态"), R { x: rcx, y: c1.y + 102.0 * s, w: rcw, h: 20.0 * s }, 12.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);

        let items: [(String, String, f32); 7] = [
            (crate::lang::t("圆角").into(), format!("{:.0}", th.radius), (th.radius - 8.0) / 28.0),
            (crate::lang::t("磨砂").into(), format!("{:.0}%", th.frost * 100.0), th.frost),
            (crate::lang::t("字号").into(), format!("{:.0}", th.font_size), (th.font_size - 13.0) / 9.0),
            (crate::lang::t("行距").into(), format!("{:.2}", th.line_height), (th.line_height - 1.35) / 1.05),
            (crate::lang::t("段距").into(), format!("{:.0}", th.para_gap), (th.para_gap - 2.0) / 42.0),
            (crate::lang::t("阴影").into(), format!("{:.0}%", th.shadow * 100.0), (th.shadow - 0.06) / 0.39),
            (crate::lang::t("纸面").into(), format!("{:.0}%", th.sheet_alpha * 100.0), ((th.sheet_alpha - 0.82) / 0.18).clamp(0.0, 1.0)),
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
            self.text_ui(rt, b, crate::lang::t("字体"), R { x: rcx, y: c2.y + 18.0 * s, w: rcw, h: 20.0 * s }, 12.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
            self.text_ui(rt, b, crate::lang::t("点一下换下一款"), R { x: rcx + rcw - 110.0 * s, y: c2.y + 18.0 * s, w: 110.0 * s, h: 20.0 * s }, 9.5 * s, 400, alpha(th.muted, 0.85), DWRITE_TEXT_ALIGNMENT_TRAILING);
            let fams: [(&str, usize); 4] = [(crate::lang::t("界面"), 0), (crate::lang::t("正文"), 1), (crate::lang::t("代码"), 2), (crate::lang::t("背景"), 3)];
            let rowh = 26.0 * s;
            let f_gap = ((c2h - 46.0 * s - 12.0 * s - rowh) / 3.0).clamp(26.0 * s, 44.0 * s);
            for (i, (label, _)) in fams.iter().enumerate() {
                let fy = c2.y + 46.0 * s + i as f32 * f_gap;
                if i == 3 {
                    // 背景行：点行 = 开/关，右边的「换图…」挑自己的图
                    let fr = R { x: rcx, y: fy, w: rcw, h: rowh };
                    let pick = R { x: fr.x + fr.w - 86.0 * s, y: fr.y + 3.0 * s, w: 62.0 * s, h: 24.0 * s };
                    self.bg_pick = R { x: pick.x - 4.0 * s, y: pick.y - 4.0 * s, w: pick.w + 8.0 * s, h: pick.h + 8.0 * s };
                    self.bg_toggle = R { x: fr.x, y: fr.y - 4.0 * s, w: fr.w - 96.0 * s, h: fr.h + 8.0 * s };
                    if self.hot == Hot::BgToggle {
                        fill_round(rt, b, fr.x, fr.y - 4.0 * s, fr.w - 96.0 * s, fr.h + 8.0 * s, 8.0 * s, alpha(th.text, 0.05));
                    }
                    self.text_ui(rt, b, label, R { x: fr.x + 4.0 * s, y: fr.y, w: 46.0 * s, h: fr.h }, 10.0 * s, 500, alpha(th.muted, 0.95), DWRITE_TEXT_ALIGNMENT_LEADING);
                    let key = self.bg_key();
                    let val = if key.is_empty() {
                        crate::lang::t("已关闭").to_string()
                    } else if key == "@builtin" {
                        crate::lang::t("内置壁纸").to_string()
                    } else {
                        std::path::Path::new(&key)
                            .file_name()
                            .map(|x| x.to_string_lossy().to_string())
                            .unwrap_or_else(|| crate::lang::t("自定义图片").to_string())
                    };
                    self.text_ui(rt, b, &val, R { x: fr.x + 58.0 * s, y: fr.y, w: fr.w - 160.0 * s, h: fr.h }, 11.0 * s, 500, if key.is_empty() { alpha(th.muted, 0.8) } else { th.text }, DWRITE_TEXT_ALIGNMENT_LEADING);
                    let hotp = self.hot == Hot::BgPick;
                    fill_round(rt, b, pick.x, pick.y, pick.w, pick.h, 8.0 * s, if hotp { alpha(th.accent, 0.16) } else { alpha(th.text, 0.05) });
                    stroke_round(rt, b, pick.x, pick.y, pick.w, pick.h, 8.0 * s, alpha(th.text, 0.08), 1.0);
                    self.text_ui(rt, b, crate::lang::t("换图…"), pick, 10.0 * s, 500, if hotp { th.accent } else { alpha(th.text, 0.8) }, DWRITE_TEXT_ALIGNMENT_CENTER);
                    continue;
                }
                let fr = R { x: rcx, y: fy, w: rcw, h: rowh };
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

            // 卡片下沿留白太多时，补两行说明把空间撑住
            let hint_y = c2.y + c2h - 62.0 * s;
            if hint_y > c2.y + 46.0 * s + 3.0 * f_gap + rowh + 16.0 * s {
                fill_r(rt, b, R { x: rcx, y: hint_y - 16.0 * s, w: rcw, h: 1.0 }, alpha(th.text, 0.06));
                self.text_ui(
                    rt,
                    b,
                    crate::lang::t("字体没装会自动回退到微软雅黑 / Segoe UI，照样能读。"),
                    R { x: rcx, y: hint_y, w: rcw, h: 18.0 * s },
                    9.5 * s,
                    400,
                    alpha(th.muted, 0.85),
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                );
                self.text_ui(
                    rt,
                    b,
                    "所有改动都写进 %APPDATA%\\Scheda\\settings.json",
                    R { x: rcx, y: hint_y + 20.0 * s, w: rcw, h: 18.0 * s },
                    9.5 * s,
                    400,
                    alpha(th.muted, 0.7),
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                );
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
                (self.chip_open, crate::lang::t("打开主题文件夹"), self.hot == Hot::OpenThemes),
                (self.chip_reset, crate::lang::t("恢复当前预设"), self.hot == Hot::ResetTheme),
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
        // 编辑态：只负责光标闪烁，一直刷
        if self.editing {
            InvalidateRect(Some(self.hwnd), None, false);
            return;
        }
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

    /// 拖正文/目录滚动条：y 是鼠标当前客户区坐标
    unsafe fn drag_scrollbar(&mut self, y: f32) {
        if self.sb_drag && self.sb_track.h > 1.0 {
            let tr = self.sb_track;
            let th_h = self.sb_thumb.h.min(tr.h);
            let ty = (y - self.sb_grab).clamp(tr.y, tr.y + (tr.h - th_h).max(0.0));
            self.sb_thumb.y = ty;
            let t = ((ty - tr.y) / (tr.h - th_h).max(1.0)).clamp(0.0, 1.0);
            let max = self.max_scroll();
            self.scroll = t * max;
            self.scroll_t = self.scroll;
        } else if self.toc_drag && self.toc_track.h > 1.0 {
            let tr = self.toc_track;
            let th_h = self.toc_thumb.h.min(tr.h);
            let ty = (y - self.toc_grab).clamp(tr.y, tr.y + (tr.h - th_h).max(0.0));
            self.toc_thumb.y = ty;
            let t = ((ty - tr.y) / (tr.h - th_h).max(1.0)).clamp(0.0, 1.0);
            self.toc_scroll = t * self.toc_max;
            self.toc_scroll_t = self.toc_scroll;
        }
    }

    /// 点轨道：把滑块中心挪到鼠标处，然后直接进入拖动
    unsafe fn grab_scrollbar(&mut self, y: f32) {
        if self.sb_track.h > 1.0 {
            let tr = self.sb_track;
            let th_h = self.sb_thumb.h.min(tr.h);
            let t = ((y - tr.y - th_h * 0.5) / (tr.h - th_h).max(1.0)).clamp(0.0, 1.0);
            let max = self.max_scroll();
            self.scroll = t * max;
            self.scroll_t = self.scroll;
            self.sb_thumb.y = tr.y + (tr.h - th_h) * t;
            self.sb_drag = true;
            self.sb_grab = th_h * 0.5;
        }
    }

    unsafe fn grab_toc(&mut self, y: f32) {
        if self.toc_track.h > 1.0 {
            let tr = self.toc_track;
            let th_h = self.toc_thumb.h.min(tr.h);
            let t = ((y - tr.y - th_h * 0.5) / (tr.h - th_h).max(1.0)).clamp(0.0, 1.0);
            self.toc_scroll = t * self.toc_max;
            self.toc_scroll_t = self.toc_scroll;
            self.toc_thumb.y = tr.y + (tr.h - th_h) * t;
            self.toc_drag = true;
            self.toc_grab = th_h * 0.5;
        }
    }

    unsafe fn wheel_at(&mut self, delta: f32, x: f32, y: f32) {
        // 图片查看器：滚轮缩放（以指针为锚点）
        if self.viewer.is_some() {
            let z0 = self.viewer.as_ref().map(|v| v.zoom).unwrap_or(1.0);
            let z = (z0 * (1.0 + delta * 0.0016)).clamp(0.12, 8.0);
            let (w, h) = (self.w as f32, self.h as f32);
            self.viewer_set_zoom(z, (x, y), w, h);
            InvalidateRect(Some(self.hwnd), None, false);
            return;
        }
        // 欢迎页：指针在「最近打开」卡片里 → 滚这张卡
        if !self.editing && !self.settings && self.file.is_none() && self.wrec_max > 1.0 && self.wrec_rect.hit(x, y) {
            self.wrec_scroll = (self.wrec_scroll - delta * 1.1 * self.scale).clamp(0.0, self.wrec_max);
            InvalidateRect(Some(self.hwnd), None, false);
            return;
        }
        // 编辑态：滚自己的缓冲
        if self.editing {
            let maxs = self.edit_max_scroll();
            let step = delta * 1.2 * self.scale;
            self.edit_scroll_t = (self.edit_scroll_t - step).clamp(0.0, maxs);
            self.edit_scroll = self.edit_scroll_t;
            InvalidateRect(Some(self.hwnd), None, false);
            return;
        }
        // 指针落在目录面板且目录确实装不下 → 滚目录，否则滚正文
        if self.toc_max > 1.0 && self.toc_panel.hit(x, y) && self.toc_panel.h > 1.0 {
            self.toc_scroll_t = (self.toc_scroll_t - delta * 0.9 * self.scale).clamp(0.0, self.toc_max);
            self.toc_scroll = self.toc_scroll_t;
            InvalidateRect(Some(self.hwnd), None, false);
            return;
        }
        self.wheel(delta);
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
        self.themes.base_mut().apply_json(&self.overrides);
        self.themes.rebuild();
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
                if self.editing {
                    self.leave_edit(true);
                }
                self.settings = !self.settings;
            }
            Hot::Lock => {
                if self.editing {
                    self.leave_edit(true);
                } else {
                    self.enter_edit();
                }
            }
            Hot::Lang(i) => {
                crate::lang::set(i);
                theme::set_setting("lang", serde_json::json!(crate::lang::code()));
                self.gfx.fmts.clear();
                self.gfx.doc_fmts.clear();
                self.doc = None;
            }
            Hot::ImageHit(_) => {
                let (mx, my) = self.mouse;
                if let Some(d) = self.doc.as_ref() {
                    if let Some((p, bmp)) = d.image_hit(mx, my) {
                        self.open_viewer(&p, bmp);
                    }
                }
            }
            Hot::ViewerFit => {
                if let Some(v) = self.viewer.as_mut() {
                    v.zoom = v.fit;
                    v.ox = 0.0;
                    v.oy = 0.0;
                }
            }
            Hot::ViewerOne => {
                if let Some(v) = self.viewer.as_mut() {
                    v.zoom = 1.0;
                    v.ox = 0.0;
                    v.oy = 0.0;
                }
            }
            Hot::ViewerCopy => {
                self.copy_viewer_image();
            }
            Hot::ViewerClose => {
                self.viewer_close();
            }
            Hot::EditText => {
                let (mx, my) = self.mouse;
                self.edit_click(mx, my);
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
                    self.themes.set_active(i);
                    self.theme_rev += 1;
                    self.gfx.bg = None;
                    self.gfx.bg_key = (0, 0, 0);
                    self.set_theme_dark_mode();
                    self.themes.save(&self.overrides);
                }
            }
            Hot::Mode(light) => {
                self.themes.set_dark(!light);
                self.theme_rev += 1;
                self.gfx.bg = None;
                self.gfx.bg_key = (0, 0, 0);
                self.set_theme_dark_mode();
                self.themes.save(&self.overrides);
            }
            Hot::BgToggle => {
                self.bg = if self.bg_key().is_empty() { "default".to_string() } else { "none".to_string() };
                theme::set_setting("bg", serde_json::json!(self.bg.clone()));
                self.gfx.bg_img = None;
                self.gfx.bg_img_path.clear();
            }
            Hot::BgPick => {
                self.pick_bg();
            }
            Hot::Assoc => {
                self.register_md();
            }
            Hot::Recent(i) => {
                if let Some(p) = self.recent.get(i).cloned() {
                    let pb = PathBuf::from(&p);
                    if pb.exists() {
                        self.open_file(&pb);
                    } else {
                        self.recent = theme::load_recent();
                        self.status = Some((crate::lang::t("这个文件已经不在了").to_string(), 2.0));
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
                self.themes.rebuild();
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
                if std::env::var_os("SCHEDA_TRACE").is_some() {
                    eprintln!("TOC click i={i} y={y} scroll={} height={:?}", self.scroll, self.doc.as_ref().map(|d| d.height));
                }
                self.scroll_to(y);
            }
            Hot::CopyCode(i) => {                if let Some(txt) = self.doc.as_ref().and_then(|d| d.code_text(i)) {
                    self.copy_text(&txt);
                    self.status = Some((crate::lang::t("已复制").into(), 1.6));
                    SetTimer(Some(self.hwnd), 1, 8, None);
                }
            }
            _ => {}
        }
        InvalidateRect(Some(self.hwnd), None, false);
    }

    unsafe fn key(&mut self, vk: u16) {
        if self.viewer.is_some() {
            let ctrl = (GetKeyState(0x11) as u32 & 0x8000) != 0;
            match vk {
                0x1B => self.viewer_close(),
                0x43 if ctrl => self.copy_viewer_image(),
                0x30 if ctrl => {
                    if let Some(v) = self.viewer.as_mut() {
                        v.zoom = v.fit;
                        v.ox = 0.0;
                        v.oy = 0.0;
                    }
                    InvalidateRect(Some(self.hwnd), None, false);
                }
                _ => {}
            }
            return;
        }
        if self.editing {
            self.edit_key(vk);
            return;
        }
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

    // ————————————————————————————————————————————————
    // 只读 / 编辑
    // ————————————————————————————————————————————————

    /// 进入编辑：把源码装进缓冲区，切成源码视图（Monospace，带光标）。
    unsafe fn enter_edit(&mut self) {
        if self.file.is_none() {
            return;
        }
        self.editing = true;
        self.settings = false;
        self.edit_buf = self.file.as_ref().map(|f| f.1.clone()).unwrap_or_default();
        // 回到上次离开编辑时的光标和滚动位置，不是每次都从第一行开始
        let n = self.edit_nchars();
        self.edit_caret = self.edit_mem_caret.min(n);
        self.edit_scroll = self.edit_mem_scroll;
        self.edit_scroll_t = self.edit_mem_scroll_t;
        self.edit_goal_col = None;
        self.edit_layout = None;
        self.edit_key = (0, 0, 0);
        self.edit_dirty = false;
        self.status = Some((crate::lang::t("编辑中 · Ctrl+S 保存").to_string(), 2.0));
        SetTimer(Some(self.hwnd), 1, 8, None);
        InvalidateRect(Some(self.hwnd), None, false);
    }

    /// 退出编辑：默认先存盘，再把新内容重新排版成阅读视图。
    unsafe fn leave_edit(&mut self, save: bool) {
        if !self.editing {
            return;
        }
        if save {
            if let Some((p, _)) = self.file.clone() {
                if std::fs::write(&p, self.edit_buf.as_bytes()).is_ok() {
                    self.status = Some((crate::lang::t("已保存").to_string(), 2.0));
                }
            }
        }
        self.editing = false;
        // 记住光标与滚动位置：下次再点锁，原地接着改
        self.edit_mem_caret = self.edit_caret;
        self.edit_mem_scroll = self.edit_scroll;
        self.edit_mem_scroll_t = self.edit_scroll_t;
        self.edit_goal_col = None;
        self.blocks = Some(md::parse(&self.edit_buf));
        if let Some(f) = self.file.as_mut() {
            f.1 = self.edit_buf.clone();
        }
        self.doc = None;
        self.edit_layout = None;
        self.edit_dirty = false;
        InvalidateRect(Some(self.hwnd), None, false);
    }

    fn edit_nchars(&self) -> usize {
        self.edit_buf.chars().count()
    }

    fn edit_byte(&self, ci: usize) -> usize {
        self.edit_buf.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(self.edit_buf.len())
    }

    fn edit_insert(&mut self, ch: char) {
        let b = self.edit_byte(self.edit_caret);
        self.edit_buf.insert(b, ch);
        self.edit_caret += 1;
        self.edit_dirty = true;
        self.edit_key = (0, 0, 0);
    }

    fn edit_backspace(&mut self) {
        if self.edit_caret == 0 {
            return;
        }
        let a = self.edit_byte(self.edit_caret - 1);
        let b = self.edit_byte(self.edit_caret);
        self.edit_buf.replace_range(a..b, "");
        self.edit_caret -= 1;
        self.edit_dirty = true;
        self.edit_key = (0, 0, 0);
    }

    fn edit_delete(&mut self) {
        if self.edit_caret >= self.edit_nchars() {
            return;
        }
        let a = self.edit_byte(self.edit_caret);
        let b = self.edit_byte(self.edit_caret + 1);
        self.edit_buf.replace_range(a..b, "");
        self.edit_dirty = true;
        self.edit_key = (0, 0, 0);
    }

    /// 光标所在行的起点（字符下标）与该行长度
    fn edit_line(&self, caret: usize) -> (usize, usize) {
        let chars: Vec<char> = self.edit_buf.chars().collect();
        let mut start = 0usize;
        let mut i = 0usize;
        while i < caret && i < chars.len() {
            if chars[i] == '\n' {
                start = i + 1;
            }
            i += 1;
        }
        let mut len = 0usize;
        while start + len < chars.len() && chars[start + len] != '\n' {
            len += 1;
        }
        (start, len)
    }

    fn edit_move_vert(&mut self, dir: i32) {
        let (start, len) = self.edit_line(self.edit_caret);
        // 上下键记住"想去的列"：短行经过时不会把列号吃掉，光标不会一路往左漂
        let col = self.edit_goal_col.unwrap_or(self.edit_caret.saturating_sub(start));
        self.edit_goal_col = Some(col);
        if dir < 0 {
            if start == 0 {
                self.edit_caret = 0;
                return;
            }
            let prev_end = start - 1;
            let (pstart, plen) = self.edit_line(prev_end);
            self.edit_caret = pstart + col.min(plen);
        } else {
            let next = start + len + 1;
            if next > self.edit_nchars() {
                return;
            }
            let (nstart, nlen) = self.edit_line(next.min(self.edit_nchars()));
            self.edit_caret = nstart + col.min(nlen);
        }
        self.edit_key = (0, 0, 0);
    }

    unsafe fn edit_key(&mut self, vk: u16) {
        let ctrl = (GetKeyState(0x11) as u32 & 0x8000) != 0;
        let n = self.edit_nchars();
        match vk {
            0x53 => {
                // Ctrl+S
                if ctrl {
                    if let Some((p, _)) = self.file.clone() {
                        if std::fs::write(&p, self.edit_buf.as_bytes()).is_ok() {
                            self.edit_dirty = false;
                            self.status = Some((crate::lang::t("已保存").to_string(), 1.6));
                            SetTimer(Some(self.hwnd), 1, 8, None);
                            InvalidateRect(Some(self.hwnd), None, false);
                        }
                    }
                }
            }
            0x1B => {
                if ctrl {
                    self.facebook_escape();
                } else {
                    self.leave_edit(true);
                }
            }
            0x0D => {
                self.edit_insert('\n');
            }
            0x09 => {
                for _ in 0..4 {
                    self.edit_insert(' ');
                }
            }
            0x08 => self.edit_backspace(),
            0x2E => self.edit_delete(),
            0x25 => {
                // ←：Ctrl 时按单词跳；否则退一个字符
                if self.edit_caret > 0 {
                    self.edit_caret -= 1;
                    if ctrl {
                        let chars: Vec<char> = self.edit_buf.chars().collect();
                        while self.edit_caret > 0 {
                            let c = chars[self.edit_caret - 1];
                            if c.is_whitespace() {
                                break;
                            }
                            self.edit_caret -= 1;
                        }
                    }
                }
                self.edit_goal_col = None;
                self.edit_key = (0, 0, 0);
            }
            0x27 => {
                if self.edit_caret < n {
                    self.edit_caret += 1;
                    if ctrl {
                        let chars: Vec<char> = self.edit_buf.chars().collect();
                        while self.edit_caret < n {
                            let c = chars[self.edit_caret];
                            if c.is_whitespace() {
                                break;
                            }
                            self.edit_caret += 1;
                        }
                    }
                }
                self.edit_goal_col = None;
                self.edit_key = (0, 0, 0);
            }
            0x26 => self.edit_move_vert(-1),
            0x28 => self.edit_move_vert(1),
            0x24 => {
                self.edit_caret = self.edit_line(self.edit_caret).0;
                self.edit_goal_col = None;
                self.edit_key = (0, 0, 0);
            }
            0x23 => {
                let (s, l) = self.edit_line(self.edit_caret);
                self.edit_caret = s + l;
                self.edit_key = (0, 0, 0);
            }
            0x22 | 0x21 => {
                // PgUp / PgDn：翻一屏
                let step = 30;
                if vk == 0x22 {
                    for _ in 0..step {
                        self.edit_move_vert(-1);
                    }
                } else {
                    for _ in 0..step {
                        self.edit_move_vert(1);
                    }
                }
            }
            _ => {}
        }
        // 让光标始终在可视区里
        self.edit_ensure_caret_visible();
        SetTimer(Some(self.hwnd), 1, 8, None);
        InvalidateRect(Some(self.hwnd), None, false);
    }

    fn facebook_escape(&mut self) {
        // Esc 不了就用 Ctrl+Esc 兜底不做事（占位，避免误关窗口）
    }

    fn edit_ensure_caret_visible(&mut self) {
        let h = self.edit_view_h();
        let cy = self.edit_caret_px.1;
        let ch = self.edit_caret_px.2.max(12.0);
        let maxs = self.edit_max_scroll();
        if cy < self.edit_scroll + 6.0 {
            self.edit_scroll = (cy - 6.0).max(0.0);
        } else if cy + ch > self.edit_scroll + h - 14.0 {
            self.edit_scroll = (cy + ch - h + 14.0).max(0.0);
        }
        self.edit_scroll = self.edit_scroll.clamp(0.0, maxs);
        self.edit_scroll_t = self.edit_scroll;
    }

    fn edit_view_h(&self) -> f32 {
        let s = self.scale;
        (self.h as f32 - TITLE_H * s - 24.0 * s - 96.0 * s).max(80.0)
    }

    fn edit_max_scroll(&self) -> f32 {
        let th = self.edit_geo().1.h;
        if let Some(l) = self.edit_layout.as_ref() {
            let mut m = DWRITE_TEXT_METRICS::default();
            if unsafe { l.GetMetrics(&mut m) }.is_ok() {
                return (m.height - th).max(0.0);
            }
        }
        0.0
    }

    fn edit_drag_to(&mut self, y: f32) {
        let maxs = self.edit_max_scroll();
        let span = (self.edit_track.h - self.edit_thumb.h).max(1.0);
        let top = (y - self.edit_grab).clamp(self.edit_track.y, self.edit_track.y + span);
        let t = ((top - self.edit_track.y) / span).clamp(0.0, 1.0);
        self.edit_scroll = t * maxs;
        self.edit_scroll_t = self.edit_scroll;
    }

    fn edit_geo(&self) -> (R, R) {
        let s = self.scale;
        let sheet = R {
            x: 14.0 * s,
            y: TITLE_H * s + 12.0 * s,
            w: (self.w as f32 - 28.0 * s).max(200.0),
            h: (self.h as f32 - TITLE_H * s - 24.0 * s).max(120.0),
        };
        let text = R { x: sheet.x + 24.0 * s, y: sheet.y + 66.0 * s, w: sheet.w - 52.0 * s, h: (sheet.h - 80.0 * s).max(60.0) };
        (sheet, text)
    }

    unsafe fn edit_layout_get(&mut self, rt: &ID2D1RenderTarget, tw: f32) -> Option<IDWriteTextLayout> {
        let s = self.scale;
        let size = (14.0 * s).round();
        let key = (tw.round() as u32, (size * 10.0) as u32, self.theme_rev as u32);
        if self.edit_key == key {
            if let Some(l) = self.edit_layout.as_ref() {
                return Some(l.clone());
            }
        }
        let fam = self.th().font_mono.clone();
        let fmt = self.gfx.format(&fam, size, 400, false)?;
        let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let u = text_w(&self.edit_buf);
        let layout = self.gfx.dwrite.CreateTextLayout(&u, &fmt, tw.max(60.0), 400000.0).ok()?;
        let _ = layout.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, size * 1.55, size + 2.0);
        self.edit_layout = Some(layout.clone());
        self.edit_key = key;
        Some(layout)
    }

    unsafe fn paint_edit(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, _w: f32, h: f32) {
        let s = self.scale;
        let (sheet, text) = self.edit_geo();
        let fill = glass(th.is_dark(), if th.is_dark() { 0.42 } else { 0.34 });
        self.frost_panel(rt, b, th, sheet, th.radius, fill, true);

        let name = self.title();
        self.text_ui(rt, b, &name, R { x: sheet.x + 24.0 * s, y: sheet.y + 16.0 * s, w: sheet.w - 240.0 * s, h: 26.0 * s }, 15.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        self.text_ui(
            rt,
            b,
            crate::lang::t("编辑中 · Ctrl+S 保存"),
            R { x: sheet.x + 24.0 * s, y: sheet.y + 40.0 * s, w: sheet.w - 200.0 * s, h: 18.0 * s },
            10.5 * s,
            400,
            alpha(th.muted, 0.9),
            DWRITE_TEXT_ALIGNMENT_LEADING,
        );
        if self.edit_dirty {
            fill_round(rt, b, sheet.x + sheet.w - 92.0 * s, sheet.y + 22.0 * s, 66.0 * s, 20.0 * s, 10.0 * s, alpha(th.accent, 0.16));
            self.text_ui(rt, b, "●", R { x: sheet.x + sheet.w - 88.0 * s, y: sheet.y + 22.0 * s, w: 20.0 * s, h: 20.0 * s }, 9.0 * s, 600, th.accent, DWRITE_TEXT_ALIGNMENT_LEADING);
        }
        fill_r(rt, b, R { x: text.x - 6.0 * s, y: text.y - 12.0 * s, w: sheet.w - 42.0 * s, h: 1.0 }, alpha(th.text, 0.07));

        let layout = match self.edit_layout_get(rt, text.w) {
            Some(l) => l,
            None => return,
        };
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = layout.GetMetrics(&mut m);
        let total_h = m.height;
        let maxs = (total_h - text.h).max(0.0);
        self.edit_scroll = self.edit_scroll.clamp(0.0, maxs);
        self.edit_scroll_t = self.edit_scroll_t.clamp(0.0, maxs);

        rt.PushAxisAlignedClip(&rectf(text.x - 4.0, text.y - 6.0, text.w + 20.0, text.h + 12.0), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        // 行底纹（每 10 行一根极淡的线，读长文不串行）
        let lh = (14.0 * s * 1.55).max(1.0);
        let first = (self.edit_scroll / lh).floor() as usize;
        let lines = (text.h / lh).ceil() as usize + 2;
        for i in 0..lines {
            let y = text.y + (first + i) as f32 * lh - self.edit_scroll;
            if (first + i) % 5 == 0 {
                fill_r(rt, b, R { x: text.x, y, w: text.w, h: 1.0 }, alpha(th.text, 0.035));
            }
        }
        b.SetColor(&cf(th.text));
        let _ = rt.DrawTextLayout(pt(text.x, text.y - self.edit_scroll), &layout, b, D2D1_DRAW_TEXT_OPTIONS_CLIP);

        // 光标
        let mut cx = 0.0f32;
        let mut cy = 0.0f32;
        let mut cm = DWRITE_HIT_TEST_METRICS::default();
        if layout.HitTestTextPosition(self.edit_caret as u32, false, &mut cx, &mut cy, &mut cm).is_ok() {
            let ch = if cm.height > 1.0 { cm.height } else { lh };
            let blink = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| (d.as_millis() / 500) % 2 == 0).unwrap_or(true);
            let y = text.y + cy - self.edit_scroll;
            if blink && y > text.y - ch && y < text.y + text.h {
                fill_round(rt, b, text.x + cx, y, 2.0 * s, ch, 0.6 * s, alpha(th.accent, 0.9));
            }
            self.edit_caret_px = (text.x + cx, cy, ch);
        }
        rt.PopAxisAlignedClip();

        // 自己的滚动条
        self.edit_track = R::default();
        self.edit_thumb = R::default();
        if maxs > 1.0 {
            let tx = sheet.x + sheet.w - 18.0 * s;
            let ty = text.y;
            let thh = text.h;
            self.edit_track = R { x: tx - 4.0 * s, y: ty, w: 12.0 * s, h: thh };
            fill_round(rt, b, tx, ty, 4.0 * s, thh, 2.0 * s, alpha(th.text, 0.05));
            let thumb = ((text.h * text.h) / total_h).max(40.0 * s).min(thh);
            let t = (self.edit_scroll / maxs).clamp(0.0, 1.0);
            let yy = ty + (thh - thumb) * t;
            fill_round(rt, b, tx, yy, 4.0 * s, thumb, 2.0 * s, alpha(th.accent, if self.edit_drag { 0.62 } else { 0.42 }));
            self.edit_thumb = R { x: tx - 4.0 * s, y: yy, w: 12.0 * s, h: thumb };
        }
        let _ = h;
    }

    /// 点在源码上 → 把光标放到最近的位置
    unsafe fn edit_click(&mut self, rt_x: f32, rt_y: f32) {
        let (_, text) = self.edit_geo();
        if let Some(layout) = self.edit_layout.clone() {
            let mut pos = 0u32;
            let mut trail = 0u32;
            let mut inside = BOOL(0);
            let lx = rt_x - text.x;
            let ly = rt_y - text.y + self.edit_scroll;
            let mut trailing = BOOL(0);
            let mut m = DWRITE_HIT_TEST_METRICS::default();
            if layout.HitTestPoint(lx, ly, &mut inside, &mut trailing, &mut m).is_ok() {
                let p0 = m.textPosition as usize;
                let p = if trailing.as_bool() { p0 + m.length as usize } else { p0 }.min(self.edit_nchars());
                self.edit_caret = p;
                self.edit_goal_col = None;
                self.edit_key = (0, 0, 0);
                SetTimer(Some(self.hwnd), 1, 8, None);
                InvalidateRect(Some(self.hwnd), None, false);
            }
        }
    }

    // ————————————————————————————————————————————————
    // 图片查看器
    // ————————————————————————————————————————————————

    unsafe fn open_viewer(&mut self, path: &std::path::Path, bmp: Option<ID2D1Bitmap>) {
        let (iw, ih) = match bmp.as_ref() {
            Some(bm) => {
                let sz = bm.GetSize();
                (sz.width, sz.height)
            }
            None => (1.0, 1.0),
        };
        let s = self.scale;
        let avail_w = (self.w as f32 - 120.0 * s).max(100.0);
        let avail_h = (self.h as f32 - 200.0 * s).max(100.0);
        let fit = if iw > 1.0 && ih > 1.0 { (avail_w / iw).min(avail_h / ih).min(1.6) } else { 1.0 };
        self.viewer = Some(Viewer { bmp, path: path.to_path_buf(), zoom: fit, ox: 0.0, oy: 0.0, drag: false, grab: (0.0, 0.0), fit });
    }

    fn viewer_close(&mut self) {
        self.viewer = None;
        unsafe { InvalidateRect(Some(self.hwnd), None, false) };
    }

    unsafe fn paint_viewer(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, w: f32, h: f32) {
        let s = self.scale;
        fill_r(rt, b, R { x: 0.0, y: 0.0, w, h }, [0.02, 0.03, 0.05, 0.72]);
        let v = match self.viewer.as_ref() {
            Some(v) => v,
            None => return,
        };
        let bmp = v.bmp.clone();
        let zoom = v.zoom;
        let (ox, oy) = (v.ox, v.oy);
        let name = v.path.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        let iw = bmp.as_ref().map(|x| x.GetSize().width * zoom).unwrap_or(0.0);
        let ih = bmp.as_ref().map(|x| x.GetSize().height * zoom).unwrap_or(0.0);
        let dx = (w - iw) * 0.5 + ox;
        let dy = (h - ih) * 0.5 + oy;

        if let Some(bm) = bmp.as_ref() {
            // 底板投影
            fill_round(rt, b, dx - 10.0 * s, dy - 8.0 * s, iw + 20.0 * s, ih + 16.0 * s, 10.0 * s, [0.0, 0.0, 0.0, 0.28]);
            let _ = rt.DrawBitmap(bm, Some(&rectf(dx, dy, iw, ih)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
        } else {
            self.text_ui(rt, b, crate::lang::t("打不开这个文件"), R { x: 0.0, y: h * 0.5, w, h: 30.0 * s }, 14.0 * s, 500, alpha(th.text, 0.8), DWRITE_TEXT_ALIGNMENT_CENTER);
        }

        // 顶部工具条（毛玻璃）
        let bar_w = (560.0 * s).min(w - 40.0 * s);
        let bar = R { x: (w - bar_w) * 0.5, y: 24.0 * s, w: bar_w, h: 46.0 * s };
        self.frost_panel(rt, b, th, bar, 23.0 * s, glass(th.is_dark(), 0.30), true);
        let zoom_txt = format!("{:.0}%", zoom * 100.0);
        self.text_ui(rt, b, &zoom_txt, R { x: bar.x + 18.0 * s, y: bar.y, w: 56.0 * s, h: bar.h }, 11.5 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        let label = if name.chars().count() > 18 { format!("{}…", name.chars().take(17).collect::<String>()) } else { name.clone() };
        self.text_ui(rt, b, &label, R { x: bar.x + 78.0 * s, y: bar.y, w: bar.w - 78.0 * s - 210.0 * s, h: bar.h }, 10.5 * s, 400, alpha(th.muted, 0.95), DWRITE_TEXT_ALIGNMENT_LEADING);
        // 按钮：适应 / 1:1 / 复制 / 关
        let bw = 56.0 * s;
        let bh = 28.0 * s;
        let by = bar.y + (bar.h - bh) * 0.5;
        let bx = bar.x + bar.w - (bw + 8.0 * s) * 4.0 + 4.0 * s;
        let btns = [
            (crate::lang::t("适应窗口"), Hot::ViewerFit),
            (crate::lang::t("原始大小"), Hot::ViewerOne),
            (crate::lang::t("复制"), Hot::ViewerCopy),
            (crate::lang::t("关闭"), Hot::ViewerClose),
        ];
        for (i, (lab, hot)) in btns.iter().enumerate() {
            let r = R { x: bx + i as f32 * (bw + 8.0 * s), y: by, w: bw, h: bh };
            self.viewer_btn_rects[i] = r;
            let on = self.hot == *hot;
            if on {
                fill_round(rt, b, r.x, r.y, r.w, r.h, 14.0 * s, alpha(th.accent, 0.14));
            } else {
                fill_round(rt, b, r.x, r.y, r.w, r.h, 14.0 * s, alpha(if th.is_dark() { [1.0, 1.0, 1.0, 1.0] } else { [0.0, 0.0, 0.0, 1.0] }, 0.045));
            }
            self.text_ui(rt, b, lab, r, 10.0 * s, 500, if on { th.accent } else { alpha(th.text, 0.8) }, DWRITE_TEXT_ALIGNMENT_CENTER);
        }
        // 底部提示（自己带一小条磨砂，不然会跟背后的正文搅在一起）
        let hint = crate::lang::t("滚轮缩放 · 拖动平移 · Ctrl+C 复制 · Esc 关闭");
        let hw = self.measure_ui(hint, 10.0 * s, 400) + 32.0 * s;
        let hr = R { x: (w - hw) * 0.5, y: h - 52.0 * s, w: hw, h: 30.0 * s };
        fill_round(rt, b, hr.x, hr.y, hr.w, hr.h, 15.0 * s, [0.02, 0.03, 0.05, 0.55]);
        self.text_ui(rt, b, hint, hr, 10.0 * s, 400, [1.0, 1.0, 1.0, 0.82], DWRITE_TEXT_ALIGNMENT_CENTER);
        self.viewer_img_rect = R { x: dx, y: dy, w: iw, h: ih };
    }

    /// 把当前图片以 CF_DIB 方式放进剪贴板（Ctrl+C）
    unsafe fn copy_viewer_image(&mut self) {
        let path = match self.viewer.as_ref() {
            Some(v) => v.path.clone(),
            None => return,
        };
        let f: IWICImagingFactory = match CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) {
            Ok(f) => f,
            Err(_) => return,
        };
        let dec = match f.CreateDecoderFromFilename(&HSTRING::from(path.to_string_lossy().to_string()), None, GENERIC_READ, WICDecodeMetadataCacheOnLoad) {
            Ok(d) => d,
            Err(_) => return,
        };
        let frame = match dec.GetFrame(0) {
            Ok(fr) => fr,
            Err(_) => return,
        };
        let conv = match f.CreateFormatConverter() {
            Ok(c) => c,
            Err(_) => return,
        };
        if conv.Initialize(&frame, &GUID_WICPixelFormat32bppBGR, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeMedianCut).is_err() {
            return;
        }
        let (mut iw, mut ih) = (0u32, 0u32);
        if conv.GetSize(&mut iw, &mut ih).is_err() {
            return;
        }
        let stride = iw * 4;
        let size = (stride * ih) as usize;
        let mut px = vec![0u8; size];
        if conv.CopyPixels(std::ptr::null(), stride, &mut px).is_err() {
            return;
        }
        // DIB 是自下而上存的，翻一下
        let mut flip = vec![0u8; size];
        for row in 0..ih as usize {
            let src = row * stride as usize;
            let dst = (ih as usize - 1 - row) * stride as usize;
            flip[dst..dst + stride as usize].copy_from_slice(&px[src..src + stride as usize]);
        }

        let bi = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: iw as i32,
            biHeight: ih as i32,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: size as u32,
            biXPelsPerMeter: 2835,
            biYPelsPerMeter: 2835,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let hdr = std::mem::size_of::<BITMAPINFOHEADER>();
        let total = hdr + size;
        if OpenClipboard(Some(self.hwnd)).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, total) {
            let p = GlobalLock(h) as *mut u8;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(&bi as *const _ as *const u8, p, hdr);
                std::ptr::copy_nonoverlapping(flip.as_ptr(), p.add(hdr), size);
                let _ = GlobalUnlock(h);
                let _ = SetClipboardData(8, Some(HANDLE(h.0)));
            }
        }
        let _ = CloseClipboard();
        self.status = Some((crate::lang::t("图片已复制").to_string(), 1.6));
        SetTimer(Some(self.hwnd), 1, 8, None);
        InvalidateRect(Some(self.hwnd), None, false);
    }

    fn viewer_set_zoom(&mut self, z: f32, anchor: (f32, f32), w: f32, h: f32) {
        if let Some(v) = self.viewer.as_mut() {
            let z = z.clamp(0.08, 8.0);
            if let Some(bm) = v.bmp.as_ref() {
                let sz = unsafe { bm.GetSize() };
                let scale_old = v.zoom;
                let iw = sz.width * scale_old;
                let ih = sz.height * scale_old;
                let cx = (w - iw) * 0.5 + v.ox;
                let cy = (h - ih) * 0.5 + v.oy;
                // 光标下的图像坐标保持不变
                let ix = (anchor.0 - cx) / scale_old.max(1e-4);
                let iy = (anchor.1 - cy) / scale_old.max(1e-4);
                v.zoom = z;
                let iw2 = sz.width * z;
                let ih2 = sz.height * z;
                v.ox = anchor.0 - ix * z - (w - iw2) * 0.5;
                v.oy = anchor.1 - iy * z - (h - ih2) * 0.5;
            }
        }
        unsafe { InvalidateRect(Some(self.hwnd), None, false) };
    }


    unsafe fn invalidate_layout(&mut self) {
        if let Some(d) = self.doc.as_mut() {
            d.key = (0, 0, 0, 0);
        }
        self.edit_key = (0, 0, 0);
        self.edit_layout = None;
        InvalidateRect(Some(self.hwnd), None, false);
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
















