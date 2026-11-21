//! 文档排版与绘制：Markdown 块模型 → DirectWrite 文本布局 → Direct2D 绘制。

use crate::*;
use crate::theme;
use md::{Align, Block, Run, Tok, TokKind};
use std::collections::HashMap;
use std::path::PathBuf;

// ————————————————————————————————————————————————————————————————
// 数据
// ————————————————————————————————————————————————————————————————

#[derive(Clone)]
pub struct Toc {
    pub level: u8,
    pub text: String,
    pub y: f32,
}

#[derive(Clone)]
pub struct Doc {
    pub base: PathBuf,
    pub title: String,
    pub meta: String,
    /// 内容总高（不含上下留白）
    pub height: f32,
    /// 可视区高度
    pub view_h: f32,
    pub toc: Vec<Toc>,
    pub copy_hits: Vec<(usize, R)>,
    pub key: (u32, u32, u32, u32),
    items: Vec<Item>,
    quotes: Vec<QuoteBox>,
}

#[derive(Clone, Copy)]
struct QuoteBox {
    x: f32,
    w: f32,
    y0: f32,
    y1: f32,
}

#[derive(Clone)]
struct Item {
    y: f32,
    h: f32,
    x: f32,
    w: f32,
    quote: u32,
    marker: Option<(Marker, f32)>,
    kind: ItemKind,
}

#[derive(Clone)]
enum Marker {
    Bullet,
    /// 序号文本（右对齐画在列表起点前）
    Num(String),
    Task(bool),
}

#[derive(Clone)]
enum ItemKind {
    Meta,
    Text(TextItem),
    Code(CodeItem),
    Table(TableItem),
    Rule,
    Image(ImageItem),
}

#[derive(Clone)]
struct TextItem {
    layout: IDWriteTextLayout,
    size: f32,
    color: Color,
    code_bg: Vec<R>,
}

#[derive(Clone)]
struct CodeItem {
    lang: String,
    src: String,
    lines: Vec<IDWriteTextLayout>,
    line_h: f32,
    pad_top: f32,
}

#[derive(Clone)]
struct TableItem {
    cols: Vec<f32>,
    aligns: Vec<Align>,
    head: Vec<Cell>,
    rows: Vec<Vec<Cell>>,
    head_h: f32,
    row_h: Vec<f32>,
}

#[derive(Clone)]
struct Cell {
    layout: IDWriteTextLayout,
    h: f32,
}

#[derive(Clone)]
struct ImageItem {
    bmp: Option<ID2D1Bitmap>,
    alt: String,
}

// ————————————————————————————————————————————————————————————————
// 排版
// ————————————————————————————————————————————————————————————————

struct Lay<'a> {
    gfx: &'a mut Gfx,
    rt: &'a ID2D1RenderTarget,
    th: Theme,
    s: f32,
    fs: f32,
    y: f32,
    items: Vec<Item>,
    toc: Vec<Toc>,
    quotes: Vec<QuoteBox>,
    qid: u32,
    imgs: HashMap<String, Option<ID2D1Bitmap>>,
    base: PathBuf,
}

fn runs_text(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        s.push_str(&r.text);
    }
    s
}

fn heading_style(level: u8, fs: f32, s: f32) -> (f32, i32, f32, f32) {
    match level {
        1 => (fs * 1.92, 700, 4.0 * s, 20.0 * s),
        2 => (fs * 1.55, 600, 30.0 * s, 12.0 * s),
        3 => (fs * 1.28, 600, 22.0 * s, 9.0 * s),
        4 => (fs * 1.13, 600, 20.0 * s, 8.0 * s),
        5 => (fs * 1.03, 600, 16.0 * s, 6.0 * s),
        _ => (fs * 0.97, 600, 14.0 * s, 6.0 * s),
    }
}

unsafe fn hit_rects(layout: &IDWriteTextLayout, start: u32, len: u32) -> Vec<R> {
    let mut out = Vec::new();
    let mut count = 0u32;
    if layout.HitTestTextRange(start, len, 0.0, 0.0, None, &mut count).is_err() || count == 0 {
        return out;
    }
    let mut ms = vec![DWRITE_HIT_TEST_METRICS::default(); count as usize];
    let mut got = 0u32;
    if layout
        .HitTestTextRange(start, len, 0.0, 0.0, Some(&mut ms), &mut got)
        .is_ok()
    {
        for m in ms.iter().take(got as usize) {
            if m.length > 0 && m.width > 0.0 {
                out.push(R { x: m.left, y: m.top, w: m.width, h: m.height });
            }
        }
    }
 out
}

impl<'a> Lay<'a> {
    fn push(&mut self, it: Item) {
        self.items.push(it);
    }

    /// 造一段带样式的文字
    unsafe fn text(
        &mut self,
        runs: &[Run],
        size: f32,
        weight: i32,
        lh: f32,
        color: Color,
        x: f32,
        w: f32,
        align: Align,
    ) -> Option<(TextItem, f32)> {
        let mut utf: Vec<u16> = Vec::new();
        for r in runs {
            utf.extend(r.text.encode_utf16());
        }
        if utf.is_empty() {
            return None;
        }
        let fam = self.th.font_text.clone();
        let fmt = self.gfx.doc_format(&fam, size, weight, false, lh)?;
        let layout = self.gfx.dwrite.CreateTextLayout(&utf, &fmt, w, 120000.0).ok()?;
        match align {
            Align::Center => {
                let _ = layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            }
            Align::Right => {
                let _ = layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING);
            }
            _ => {}
        }
        let mono = self.th.font_mono.clone();
        if std::env::var_os("SCHEDA_TRACE").is_some() {
            for r in runs {
                eprintln!("RUN bold={} code={} text={:?}", r.s.bold, r.s.code, r.text);
            }
        }
        let mut off = 0u32;
        for r in runs {
            let len = r.text.encode_utf16().count() as u32;
            if len == 0 {
                continue;
            }
            let range = DWRITE_TEXT_RANGE { startPosition: off, length: len };
            let mut col = color;
            let mut sz = size;
            let mut wgt = weight;
            if r.s.code {
                sz = size * 0.9;
                col = self.th.code_text;
                let _ = layout.SetFontFamilyName(&HSTRING::from(mono.clone()), range);
            }
            if r.s.bold {
                wgt = 700;
            }
            if let Some(_) = r.href {
                col = self.th.link;
                let _ = layout.SetUnderline(true, range);
            }
            if r.s.strike {
                let _ = layout.SetStrikethrough(true, range);
            }
            if let Some(br) = self.gfx.tbrush(self.rt, col) {
                let _ = layout.SetDrawingEffect(&br, range);
            }
            if (sz - size).abs() > 0.01 {
                let _ = layout.SetFontSize(sz, range);
            }
            // 每个 run 都显式写一次字重：DirectWrite 的 range 属性在部分字体上会往后渗，
            // 统一写死可以保证后面的文字一定回到正常字重。
            let _ = layout.SetFontWeight(fweight(wgt), range);
            if r.s.italic {
                let _ = layout.SetFontStyle(DWRITE_FONT_STYLE_ITALIC, range);
            }
            off += len;
        }
        let mut m = DWRITE_TEXT_METRICS::default();
        if layout.GetMetrics(&mut m).is_err() {
            return None;
        }
        // 行内代码的“药丸”底
        let mut code_bg = Vec::new();
        let mut off = 0u32;
        for r in runs {
            let len = r.text.encode_utf16().count() as u32;
            if len > 0 && r.s.code {
                for hr in hit_rects(&layout, off, len) {
                    let ph = (size * 1.42).min(hr.h).max(size * 1.05);
                    code_bg.push(R {
                        x: hr.x - 2.0 * self.s,
                        y: hr.y + (hr.h - ph) * 0.5,
                        w: hr.w + 4.0 * self.s,
                        h: ph,
                    });
                }
            }
            off += len;
        }
        Some((TextItem { layout, size, color, code_bg }, m.height))
    }

    unsafe fn measure(&mut self, text: &str, size: f32, weight: i32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let fam = self.th.font_text.clone();
        if let Some(fmt) = self.gfx.doc_format(&fam, size, weight, false, 1.4) {
            let u = text_w(text);
            if let Ok(l) = self.gfx.dwrite.CreateTextLayout(&u, &fmt, 40000.0, 4000.0) {
                let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                let mut m = DWRITE_TEXT_METRICS::default();
                if l.GetMetrics(&mut m).is_ok() {
                    return m.widthIncludingTrailingWhitespace;
                }
            }
        }
        text.chars().count() as f32 * size * 0.58
    }
}

fn lay_blocks(c: &mut Lay, blocks: &[Block], x: f32, w: f32, quote: u32) {
    let s = c.s;
    let fs = c.fs;
    for b in blocks {
        match b {
            Block::Heading { level, runs } => {
                let (size, weight, before, after) = heading_style(*level, fs, s);
                c.y += before;
                if let Some((it, h)) = unsafe { c.text(runs, size, weight, 1.24, c.th.heading.clone(), x, w, Align::None) } {
                    c.toc.push(Toc { level: *level, text: runs_text(runs), y: c.y });
                    c.push(Item { y: c.y, h, x, w, quote, marker: None, kind: ItemKind::Text(it) });
                    c.y += h + after;
                }
            }
            Block::Para { runs } => {
                let lh = c.th.line_height;
                if let Some((it, h)) = unsafe { c.text(runs, fs, 400, lh, c.th.text.clone(), x, w, Align::None) } {
                    c.push(Item { y: c.y, h, x, w, quote, marker: None, kind: ItemKind::Text(it) });
                    c.y += h + c.th.para_gap * s;
                }
            }
            Block::Code { lang, lines } => {
                unsafe { lay_code(c, lang, lines, x, w, quote) };
            }
            Block::Quote { blocks } => {
                c.qid += 1;
                let q = c.qid;
                let y0 = c.y;
                c.y += 4.0 * s;
                let inner_x = x + 20.0 * s;
                let inner_w = (w - 22.0 * s).max(80.0);
                lay_blocks(c, blocks, inner_x, inner_w, q);
                c.y += 6.0 * s;
                c.quotes.push(QuoteBox { x: x + 4.0 * s, w: w - 4.0 * s, y0, y1: c.y });
            }
            Block::List { ordered, start, items } => {
                for (i, item) in items.iter().enumerate() {
                    let mark = match item.task {
                        Some(b) => Marker::Task(b),
                        None => {
                            if *ordered {
                                let label = format!("{}.", start + i as u64);
                                unsafe { lay_num(c, &label, x, fs, s) }
                            } else {
                                Marker::Bullet
                            }
                        }
                    };
                    let idx = c.items.len();
                    let ind = 27.0 * s;
                    lay_blocks(c, &item.blocks, x + ind, (w - ind).max(60.0), quote);
                    if idx < c.items.len() {
                        c.items[idx].marker = Some((mark, x));
                    }
                }
                c.y += 6.0 * s;
            }
            Block::Table { aligns, head, rows } => {
                unsafe { lay_table(c, aligns, head, rows, x, w, quote) };
            }
            Block::Rule => {
                c.y += 22.0 * s;
                c.push(Item { y: c.y, h: 1.0 * s, x, w, quote, marker: None, kind: ItemKind::Rule });
                c.y += 24.0 * s;
            }
            Block::Image { src, alt } => {
                unsafe { lay_image(c, src, alt, x, w, quote) };
            }
        }
    }
}

unsafe fn lay_num(_c: &mut Lay, label: &str, _x: f32, _fs: f32, _s: f32) -> Marker {
    Marker::Num(label.to_string())
}

unsafe fn lay_code(c: &mut Lay, lang: &str, lines: &[Vec<Tok>], x: f32, w: f32, quote: u32) {
    let s = c.s;
    let fs = c.fs;
    let size = fs * 0.9;
    let lh = 1.62;
    let line_h = size * lh;
    let pad_x = 16.0 * s;
    let has_label = !lang.trim().is_empty() || true;
    let pad_top = if has_label { 38.0 * s } else { 16.0 * s };
    let pad_bot = 16.0 * s;

    c.y += 8.0 * s;

    let mono = c.th.font_mono.clone();
    let fmt = match c.gfx.doc_format(&mono, size, 400, false, lh) {
        Some(f) => f,
        None => return,
    };
    let mut layouts = Vec::new();
    let mut src = String::new();
    for toks in lines {
        let mut u: Vec<u16> = Vec::new();
        let mut line_txt = String::new();
        for t in toks {
            u.extend(t.t.encode_utf16());
            line_txt.push_str(&t.t);
        }
        if toks.is_empty() || u.is_empty() {
            u = text_w(" ");
        }
        if let Ok(l) = c.gfx.dwrite.CreateTextLayout(&u, &fmt, 40000.0, line_h * 2.0) {
            let _ = l.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let mut off = 0u32;
            for t in toks {
                let len = t.t.encode_utf16().count() as u32;
                if len == 0 {
                    continue;
                }
                let range = DWRITE_TEXT_RANGE { startPosition: off, length: len };
                let col = match t.k {
                    TokKind::Keyword => c.th.tok_keyword,
                    TokKind::Str => c.th.tok_string,
                    TokKind::Comment => c.th.tok_comment,
                    TokKind::Num => c.th.tok_number,
                    TokKind::Ty => c.th.tok_type,
                    TokKind::Punct => alpha(c.th.code_text, 0.62),
                    TokKind::Plain => c.th.code_text,
                };
                if let Some(br) = c.gfx.tbrush(c.rt, col) {
                    let _ = l.SetDrawingEffect(&br, range);
                }
                off += len;
            }
            layouts.push(l);
        }
        src.push_str(&line_txt);
        src.push('\n');
    }

    let n = layouts.len();
    let h = pad_top + n as f32 * line_h + pad_bot;
    c.push(Item {
        y: c.y,
        h,
        x,
        w,
        quote,
        marker: None,
        kind: ItemKind::Code(CodeItem { lang: lang.to_string(), src, lines: layouts, line_h, pad_top }),
    });
    c.y += h + 16.0 * s;
}

unsafe fn lay_table(c: &mut Lay, aligns: &[Align], head: &[Vec<Run>], rows: &[Vec<Vec<Run>>], x: f32, w: f32, quote: u32) {
    let s = c.s;
    let fs = c.fs;
    let size = fs * 0.9;
    let pad_x = 13.0 * s;
    let pad_y = 9.0 * s;
    let ncol = aligns.len().max(head.len()).max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if ncol == 0 {
        return;
    }
    c.y += 8.0 * s;

    // 1) 量每列自然宽度
    let mut natural = vec![0.0f32; ncol];
    for (j, cell) in head.iter().enumerate() {
        natural[j] = natural[j].max(c.measure(&runs_text(cell), size, 600));
    }
    for row in rows {
        for (j, cell) in row.iter().enumerate() {
            natural[j] = natural[j].max(c.measure(&runs_text(cell), size, 400));
        }
    }
    let total: f32 = natural.iter().map(|v| v + pad_x * 2.0).sum();
    let mut cols: Vec<f32> = Vec::new();
    if total <= w {
        let extra = (w - total) / ncol as f32;
        for v in natural.iter() {
            cols.push(v + pad_x * 2.0 + extra);
        }
    } else {
        let scale = w / total;
        for v in natural.iter() {
            cols.push(((v + pad_x * 2.0) * scale).max(52.0 * s));
        }
        let sum: f32 = cols.iter().sum();
        if sum > w {
            let f = w / sum;
            for v in cols.iter_mut() {
                *v *= f;
            }
        }
    }

    // 2) 造单元
    let mono_fmt_fam = c.th.font_text.clone();
    let build_cell = |c: &mut Lay, runs: &[Run], cw: f32, weight: i32, al: Align| -> Option<Cell> {
        let mut utf: Vec<u16> = Vec::new();
        for r in runs {
            utf.extend(r.text.encode_utf16());
        }
        if utf.is_empty() {
            utf = text_w(" ");
        }
        let fmt = c.gfx.doc_format(&mono_fmt_fam, size, weight, false, 1.42)?;
        let l = c.gfx.dwrite.CreateTextLayout(&utf, &fmt, (cw - pad_x * 2.0).max(8.0), 4000.0).ok()?;
        match al {
            Align::Center => {
                let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            }
            Align::Right => {
                let _ = l.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_TRAILING);
            }
            _ => {}
        }
        let mut off = 0u32;
        for r in runs {
            let len = r.text.encode_utf16().count() as u32;
            if len == 0 {
                continue;
            }
            let range = DWRITE_TEXT_RANGE { startPosition: off, length: len };
            let col = if weight >= 600 { c.th.heading } else if r.href.is_some() { c.th.link } else { c.th.text };
            if let Some(br) = c.gfx.tbrush(c.rt, col) {
                let _ = l.SetDrawingEffect(&br, range);
            }
            if r.s.bold && weight < 600 {
                let _ = l.SetFontWeight(DWRITE_FONT_WEIGHT_SEMI_BOLD, range);
            }
            if r.href.is_some() {
                let _ = l.SetUnderline(true, range);
            }
            off += len;
        }
        let mut m = DWRITE_TEXT_METRICS::default();
        if l.GetMetrics(&mut m).is_err() {
            return None;
        }
        Some(Cell { layout: l, h: m.height })
    };

    let al_at = |j: usize| -> Align { aligns.get(j).copied().unwrap_or(Align::None) };

    let mut hcells: Vec<Cell> = Vec::new();
    let mut head_h = 0.0f32;
    for j in 0..ncol {
        let runs: &[Run] = head.get(j).map(|v| v.as_slice()).unwrap_or(&[]);
        let cell = build_cell(c, runs, cols[j], 600, al_at(j));
        if let Some(cl) = &cell {
            head_h = head_h.max(cl.h);
        }
        hcells.push(cell.unwrap_or_else(|| Cell { layout: build_cell(c, &[], cols[j], 400, Align::None).unwrap().layout, h: size }));
    }
    head_h += pad_y * 2.0;

    let mut trows: Vec<Vec<Cell>> = Vec::new();
    let mut row_h: Vec<f32> = Vec::new();
    for row in rows {
        let mut cells = Vec::new();
        let mut rh = 0.0f32;
        for j in 0..ncol {
            let runs: &[Run] = row.get(j).map(|v| v.as_slice()).unwrap_or(&[]);
            let cell = build_cell(c, runs, cols[j], 400, al_at(j));
            if let Some(cl) = &cell {
                rh = rh.max(cl.h);
            }
            cells.push(cell.unwrap_or_else(|| Cell { layout: build_cell(c, &[], cols[j], 400, Align::None).unwrap().layout, h: size }));
        }
        row_h.push(rh + pad_y * 2.0);
        trows.push(cells);
    }

    let h: f32 = head_h + row_h.iter().sum::<f32>();
    c.push(Item {
        y: c.y,
        h,
        x,
        w,
        quote,
        marker: None,
        kind: ItemKind::Table(TableItem { cols, aligns: aligns.to_vec(), head: hcells, rows: trows, head_h, row_h }),
    });
    c.y += h + 18.0 * s;
}

unsafe fn resolve_path(base: &PathBuf, src: &str) -> Option<PathBuf> {
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") {
        return None;
    }
    let p = PathBuf::from(src);
    if p.is_absolute() {
        return Some(p);
    }
    let mut b = base.clone();
    if b.extension().is_some() {
        b.pop();
    }
    Some(b.join(src))
}

unsafe fn load_bmp(rt: &ID2D1RenderTarget, path: &PathBuf) -> Option<ID2D1Bitmap> {
    use windows::Win32::Graphics::Imaging::*;
    let wic: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
    let hs = HSTRING::from(path.to_string_lossy().to_string());
    let dec = wic.CreateDecoderFromFilename(&hs, None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
    let frame = dec.GetFrame(0).ok()?;
    let conv = wic.CreateFormatConverter().ok()?;
    conv.Initialize(&frame, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)
        .ok()?;
    rt.CreateBitmapFromWicBitmap(&conv, None).ok()
}

unsafe fn lay_image(c: &mut Lay, src: &str, alt: &str, x: f32, w: f32, quote: u32) {
    let s = c.s;
    c.y += 10.0 * s;
    if c.imgs.get(src).is_none() {
        let bmp = match resolve_path(&c.base, src) {
            Some(p) => load_bmp(c.rt, &p),
            None => None,
        };
        c.imgs.insert(src.to_string(), bmp);
    }
    let bmp = c.imgs.get(src).cloned().flatten();
    let mut iw = 0.0f32;
    let mut ih = 0.0f32;
    if let Some(b) = &bmp {
        let sz = b.GetSize();
        iw = sz.width;
        ih = sz.height;
        if iw > 0.0 && ih > 0.0 {
            let k = (w / iw).min(1.0).min((560.0 * s) / ih);
            iw *= k;
            ih *= k;
        }
    }
    let h = if bmp.is_some() { ih } else { 84.0 * s };
    c.push(Item {
        y: c.y,
        h,
        x,
        w: if bmp.is_some() { iw } else { w },
        quote,
        marker: None,
        kind: ItemKind::Image(ImageItem { bmp, alt: alt.to_string() }),
    });
    c.y += h + 18.0 * s;
}

// ————————————————————————————————————————————————————————————————
// App 接口
// ————————————————————————————————————————————————————————————————

impl App {
    /// 版心几何：纸面 / 目录面板 / 可视高度
    pub(crate) fn doc_geo(&self) -> (R, Option<R>, f32) {
        let s = self.scale;
        let w = self.w as f32;
        let h = self.h as f32;
        let toc_w = if self.toc_open && self.file.is_some() && w >= 1060.0 * s { 216.0 * s } else { 0.0 };
        let gap = if toc_w > 0.0 { 14.0 * s } else { 0.0 };
        let left = 14.0 * s + toc_w + gap;
        let right = 14.0 * s;
        let avail = (w - left - right).max(280.0);
        // 版心撑满窗口，不再收成窄条
        let sheet = R { x: left, y: TITLE_H * s + 12.0 * s, w: avail, h: (h - TITLE_H * s - 24.0 * s).max(120.0) };
        let toc = if toc_w > 0.0 { Some(R { x: 14.0 * s, y: sheet.y, w: toc_w, h: sheet.h }) } else { None };
        (sheet, toc, sheet.h)
    }

    pub(crate) fn pad_x(&self) -> f32 {
        40.0 * self.scale
    }

    fn doc_key(&self) -> (u32, u32, u32, u32) {
        let (_, _, _) = self.doc_geo();
        let text_w = (self.doc_geo().0.w - self.pad_x() * 2.0).round() as u32;
        (text_w, (self.zoom * 100.0) as u32, self.theme_rev as u32, self.file.is_some() as u32)
    }

    pub(crate) unsafe fn ensure_doc(&mut self, rt: &ID2D1RenderTarget) {
        let key = self.doc_key();
        if let Some(d) = self.doc.as_ref() {
            if d.key == key {
                return;
            }
        }
        let blocks = match self.blocks.clone() {
            Some(b) => b,
            None => return,
        };
        let (_, _, view_h) = self.doc_geo();
        let s = self.scale;
        let th = self.th().clone();
        let fs = th.font_size * s * self.zoom;
        let text_w = (self.doc_geo().0.w - self.pad_x() * 2.0).max(120.0);
        let (title, base, meta) = match &self.file {
            Some((p, txt)) => {
                let name = p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                let chars = txt.chars().filter(|c| !c.is_whitespace()).count();
                let mins = (chars as f32 / 380.0).ceil().max(1.0) as u32;
                (name, p.clone(), format!("{} 字 · 约 {} 分钟", chars, mins))
            }
            None => continue_none(),
        };

        let mut c = Lay {
            gfx: &mut self.gfx,
            rt,
            th,
            s,
            fs,
            y: 0.0,
            items: Vec::new(),
            toc: Vec::new(),
            quotes: Vec::new(),
            qid: 0,
            imgs: HashMap::new(),
            base: base.clone(),
        };

        // 顶部写出处
        let meta_h = 64.0 * s;
        c.push(Item { y: 0.0, h: meta_h, x: 0.0, w: text_w, quote: 0, marker: None, kind: ItemKind::Meta });
        c.y = meta_h;

        lay_blocks(&mut c, &blocks, 0.0, text_w, 0);

        let height = c.y + 26.0 * s;
        self.doc = Some(Doc {
            base,
            title,
            meta,
            height,
            view_h,
            toc: c.toc,
            copy_hits: Vec::new(),
            key,
            items: c.items,
            quotes: c.quotes,
        });
    }

    pub(crate) unsafe fn paint_document(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, _w: f32, _h: f32) {
        self.ensure_doc(rt);
        let s = self.scale;
        let (sheet, toc_r, _vh) = self.doc_geo();
        let scroll = self.scroll;

        // 纸面
        let fill = if th.is_dark() {
            alpha([1.0, 1.0, 1.0, 1.0], th.sheet_alpha)
        } else {
            theme::over([1.0, 1.0, 1.0, 1.0], th.bg_bottom, th.sheet_alpha.max(0.86))
        };
        self.frost_panel(rt, b, th, sheet, th.radius, fill, true);

        let pad_x = self.pad_x();
        let pad_top = 34.0 * s;
        let cx = sheet.x + pad_x;
        let oy = sheet.y + pad_top - scroll;
        let text_w = (sheet.w - pad_x * 2.0).max(80.0);

        rt.PushAxisAlignedClip(&rectf(sheet.x + 1.0, sheet.y + 1.0, sheet.w - 2.0, sheet.h - 2.0), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);

        // 引用块底
        if let Some(doc) = self.doc.as_ref() {
            for q in doc.quotes.iter() {
                let y0 = oy + q.y0;
                let height = q.y1 - q.y0;
                if y0 + height < sheet.y - 4.0 || y0 > sheet.y + sheet.h + 4.0 {
                    continue;
                }
                let qx = cx + q.x;
                let qw = (q.w).min(text_w + 8.0);
                fill_round(rt, b, qx, y0, qw, height, 8.0 * s, alpha(th.accent, 0.045));
                fill_round(rt, b, qx, y0 + 4.0 * s, 3.0 * s, (height - 8.0 * s).max(2.0), 1.5 * s, alpha(th.accent, 0.5));
            }
        }

        let mut copy_hits: Vec<(usize, R)> = Vec::new();
        let n = self.doc.as_ref().map(|d| d.items.len()).unwrap_or(0);
        for i in 0..n {
            let it = match self.doc.as_ref().and_then(|d| d.items.get(i)) {
                Some(it) => it.clone(),
                None => continue,
            };
            let iy = oy + it.y;
            if iy + it.h < sheet.y - 6.0 || iy > sheet.y + sheet.h + 6.0 {
                continue;
            }
            let ix = cx + it.x;
            match &it.kind {
                ItemKind::Meta => {
                    let title = self.doc.as_ref().map(|d| d.title.clone()).unwrap_or_default();
                    let meta = self.doc.as_ref().map(|d| d.meta.clone()).unwrap_or_default();
                    self.text_ui(rt, b, &title, R { x: cx, y: iy + 2.0 * s, w: text_w, h: 30.0 * s }, 19.0 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
                    self.text_ui(rt, b, &meta, R { x: cx, y: iy + 30.0 * s, w: text_w, h: 18.0 * s }, 11.0 * s, 400, alpha(th.muted, 0.85), DWRITE_TEXT_ALIGNMENT_LEADING);
                    fill_r(rt, b, R { x: cx, y: iy + it.h - 10.0 * s, w: text_w, h: 1.0 }, alpha(th.text, 0.07));
                }
                ItemKind::Text(t) => {
                    for pill in t.code_bg.iter() {
                        fill_round(rt, b, ix + pill.x, iy + pill.y, pill.w, pill.h, 4.0 * s, if th.is_dark() { alpha(th.code_bg, 0.85) } else { th.code_bg });
                    }
                    b.SetColor(&cf(t.color));
                    let _ = rt.DrawTextLayout(pt(ix, iy), &t.layout, b, D2D1_DRAW_TEXT_OPTIONS_CLIP);
                    let _ = t.size;
                }
                ItemKind::Code(cd) => {
                    fill_round(rt, b, ix, iy, it.w, it.h, 10.0 * s, if th.is_dark() { alpha(th.code_bg, 0.72) } else { th.code_bg });
                    stroke_round(rt, b, ix, iy, it.w, it.h, 10.0 * s, alpha(th.text, 0.09), 1.0);
                    // 语言标签
                    let label = if cd.lang.trim().is_empty() { "code" } else { cd.lang.trim() };
                    self.text_ui(rt, b, &label.to_uppercase(), R { x: ix + 16.0 * s, y: iy + 9.0 * s, w: 120.0 * s, h: 16.0 * s }, 9.5 * s, 600, alpha(th.muted, 0.85), DWRITE_TEXT_ALIGNMENT_LEADING);
                    fill_r(rt, b, R { x: ix + 14.0 * s, y: iy + 27.0 * s, w: it.w - 28.0 * s, h: 1.0 }, alpha(th.text, 0.07));
                    // 复制
                    let cr = R { x: ix + it.w - 70.0 * s, y: iy + 8.0 * s, w: 56.0 * s, h: 20.0 * s };
                    let hot = self.hot == Hot::CopyCode(i);
                    if hot {
                        fill_round(rt, b, cr.x, cr.y, cr.w, cr.h, 10.0 * s, alpha(th.accent, 0.16));
                    }
                    self.text_ui(rt, b, "复制", R { x: cr.x, y: cr.y, w: cr.w, h: cr.h }, 10.0 * s, 500, if hot { th.accent } else { alpha(th.muted, 0.8) }, DWRITE_TEXT_ALIGNMENT_CENTER);
                    copy_hits.push((i, cr));
                    for (k, line) in cd.lines.iter().enumerate() {
                        let ly = iy + cd.pad_top + k as f32 * cd.line_h;
                        if ly + cd.line_h < sheet.y - 8.0 || ly > sheet.y + sheet.h + 8.0 {
                            continue;
                        }
                        b.SetColor(&cf(th.code_text));
                        let _ = rt.DrawTextLayout(pt(ix + 18.0 * s, ly), line, b, D2D1_DRAW_TEXT_OPTIONS_CLIP);
                    }
                }
                ItemKind::Table(t) => {
                    let total_h = it.h;
                    fill_round(rt, b, ix, iy, it.w, total_h, 10.0 * s, alpha(th.text, if th.is_dark() { 0.035 } else { 0.028 }));
                    stroke_round(rt, b, ix, iy, it.w, total_h, 10.0 * s, alpha(th.text, 0.10), 1.0);
                    // 表头
                    rt.PushAxisAlignedClip(&rectf(ix + 1.0, iy + 1.0, it.w - 2.0, t.head_h - 1.0), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    fill_round(rt, b, ix, iy, it.w, total_h, 10.0 * s, alpha(th.accent, 0.075));
                    rt.PopAxisAlignedClip();
                    let mut xoff = 0.0;
                    for (j, cell) in t.head.iter().enumerate() {
                        let cw = *t.cols.get(j).unwrap_or(&0.0);
                        b.SetColor(&cf(th.heading));
                        let _ = rt.DrawTextLayout(pt(ix + xoff + 13.0 * s, iy + 9.0 * s), &cell.layout, b, D2D1_DRAW_TEXT_OPTIONS_CLIP);
                        xoff += cw;
                    }
                    let mut yy = iy + t.head_h;
                    fill_r(rt, b, R { x: ix + 1.0, y: yy, w: it.w - 2.0, h: 1.0 }, alpha(th.text, 0.10));
                    for (ri, row) in t.rows.iter().enumerate() {
                        let rh = *t.row_h.get(ri).unwrap_or(&0.0);
                        let mut xoff = 0.0;
                        for (j, cell) in row.iter().enumerate() {
                            let cw = *t.cols.get(j).unwrap_or(&0.0);
                            b.SetColor(&cf(th.text));
                            let _ = rt.DrawTextLayout(pt(ix + xoff + 13.0 * s, yy + 9.0 * s), &cell.layout, b, D2D1_DRAW_TEXT_OPTIONS_CLIP);
                            xoff += cw;
                        }
                        yy += rh;
                        if ri + 1 < t.rows.len() {
                            fill_r(rt, b, R { x: ix + 1.0, y: yy, w: it.w - 2.0, h: 1.0 }, alpha(th.text, 0.055));
                        }
                    }
                }
                ItemKind::Rule => {
                    fill_r(rt, b, R { x: ix, y: iy, w: it.w, h: 1.0 }, alpha(th.text, 0.12));
                }
                ItemKind::Image(im) => {
                    if let Some(bmp) = &im.bmp {
                        fill_round(rt, b, ix, iy, it.w, it.h, 8.0 * s, alpha(th.text, 0.05));
                        let _ = rt.DrawBitmap(
                            bmp,
                            Some(&rectf(ix, iy, it.w, it.h)),
                            1.0,
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            None,
                        );
                        stroke_round(rt, b, ix, iy, it.w, it.h, 8.0 * s, alpha(th.text, 0.08), 1.0);
                    } else {
                        fill_round(rt, b, ix, iy, it.w, it.h, 10.0 * s, alpha(th.text, if th.is_dark() { 0.04 } else { 0.03 }));
                        stroke_round(rt, b, ix, iy, it.w, it.h, 10.0 * s, alpha(th.text, 0.09), 1.0);
                        let msg = if im.alt.trim().is_empty() { "零联网 · 远程图片已跳过".to_string() } else { format!("零联网 · 未加载：{}", im.alt) };
                        self.text_ui(rt, b, &msg, R { x: ix, y: iy + it.h * 0.5 - 11.0 * s, w: it.w, h: 22.0 * s }, 11.5 * s, 400, alpha(th.muted, 0.85), DWRITE_TEXT_ALIGNMENT_CENTER);
                    }
                }
            }

            // 列表标记
            if let Some((m, mx)) = &it.marker {
                let my = iy;
                match m {
                    Marker::Bullet => {
                        let d = 5.5 * s;
                        let dy = my + fs_marker(&it) * 0.56;
                        fill_round(rt, b, cx + mx + 1.0 * s, dy, d, d, d * 0.5, alpha(th.accent, 0.72));
                    }
                    Marker::Num(label) => {
                        let fs_m = fs_marker(&it);
                        self.text_ui(
                            rt,
                            b,
                            label,
                            R { x: cx + mx - 6.0 * s, y: my, w: 26.0 * s, h: fs_m * 1.55 },
                            fs_m * 0.9,
                            500,
                            alpha(th.accent, 0.9),
                            DWRITE_TEXT_ALIGNMENT_TRAILING,
                        );
                    }
                    Marker::Task(checked) => {
                        let bx = cx + mx + 1.0 * s;
                        let by = my + fs_marker(&it) * 0.5;
                        let bs = 13.0 * s;
                        if *checked {
                            fill_round(rt, b, bx, by, bs, bs, 4.0 * s, alpha(th.accent, 0.85));
                            fill_round(rt, b, bx + 3.2 * s, by + 3.4 * s, bs - 6.4 * s, 2.0 * s, 1.0, [1.0, 1.0, 1.0, 0.95]);
                        } else {
                            stroke_round(rt, b, bx, by, bs, bs, 4.0 * s, alpha(th.muted, 0.7), 1.2);
                        }
                    }
                }
            }
        }

        rt.PopAxisAlignedClip();

        if let Some(d) = self.doc.as_mut() {
            d.copy_hits = copy_hits;
        }

        // 滚动条
        if let Some(d) = self.doc.as_ref() {
            let maxs = (d.height + pad_top + 24.0 * s - sheet.h).max(0.0);
            if maxs > 1.0 {
                let track_x = sheet.x + sheet.w - 18.0 * s;
                let track_y = sheet.y + 14.0 * s;
                let track_h = sheet.h - 28.0 * s;
                fill_round(rt, b, track_x, track_y, 4.0 * s, track_h, 2.0 * s, alpha(th.text, 0.05));
                let thumb_h = ((sheet.h * sheet.h) / (sheet.h + maxs)).max(46.0 * s).min(track_h);
                let t = (scroll / maxs).clamp(0.0, 1.0);
                let ty = track_y + (track_h - thumb_h) * t;
                fill_round(rt, b, track_x, ty, 4.0 * s, thumb_h, 2.0 * s, alpha(th.accent, 0.42));
            }
        }

        // 目录
        if let Some(tr) = toc_r {
            self.paint_toc(rt, b, th, tr, sheet);
        }
    }

    unsafe fn paint_toc(&mut self, rt: &ID2D1RenderTarget, b: &ID2D1SolidColorBrush, th: &Theme, r: R, sheet: R) {
        let s = self.scale;
        let entries: Vec<(u8, String, f32)> = self
            .doc
            .as_ref()
            .map(|d| d.toc.iter().map(|t| (t.level, t.text.clone(), t.y)).collect())
            .unwrap_or_default();
        if entries.is_empty() {
            self.toc_entries.clear();
            return;
        }
        let want_h = (58.0 * s + entries.len() as f32 * 26.0 * s + 12.0 * s).min(r.h);
        let r = R { h: want_h, ..r };
        let fill = if th.is_dark() {
            alpha([1.0, 1.0, 1.0, 1.0], 0.08)
        } else {
            theme::over([1.0, 1.0, 1.0, 1.0], th.bg_bottom, 0.94)
        };
        self.frost_panel(rt, b, th, r, th.radius, fill, true);
        self.text_ui(rt, b, "目录", R { x: r.x + 18.0 * s, y: r.y + 16.0 * s, w: 120.0 * s, h: 22.0 * s }, 12.5 * s, 600, th.heading, DWRITE_TEXT_ALIGNMENT_LEADING);
        fill_r(rt, b, R { x: r.x + 14.0 * s, y: r.y + 44.0 * s, w: r.w - 28.0 * s, h: 1.0 }, alpha(th.text, 0.08));

        let cur = self.scroll + 50.0 * s;
        let mut active = 0usize;
        for (i, e) in entries.iter().enumerate() {
            if e.2 <= cur {
                active = i;
            }
        }

        let mut y = r.y + 56.0 * s;
        let bottom = r.y + r.h - 12.0 * s;
        let mut hits: Vec<R> = Vec::new();
        rt.PushAxisAlignedClip(&rectf(r.x + 2.0, r.y + 46.0 * s, r.w - 4.0, r.h - 48.0 * s), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        for (i, e) in entries.iter().enumerate() {
            if y + 26.0 * s > bottom {
                break;
            }
            let indent = 16.0 + (e.0.saturating_sub(1) as f32) * 10.0;
            let row = R { x: r.x + 10.0 * s, y, w: r.w - 20.0 * s, h: 26.0 * s };
            if i == active {
                fill_round(rt, b, row.x, row.y + 2.0 * s, row.w, row.h - 4.0 * s, 7.0 * s, alpha(th.accent, 0.10));
                fill_round(rt, b, row.x, row.y + 7.0 * s, 2.5 * s, row.h - 14.0 * s, 1.25 * s, alpha(th.accent, 0.85));
            }
            let col = if i == active { th.heading } else { alpha(th.text, 0.78) };
            let size = if e.0 <= 1 { 12.5 * s } else { 11.8 * s };
            self.text_ui(
                rt,
                b,
                &e.1,
                R { x: row.x + indent * s, y, w: row.w - indent * s - 12.0 * s, h: 26.0 * s },
                size,
                500,
                col,
                DWRITE_TEXT_ALIGNMENT_LEADING,
            );
            hits.push(row);
            y += 26.0 * s;
        }
        rt.PopAxisAlignedClip();
        let _ = sheet;
        self.toc_entries = hits;
    }

    pub(crate) unsafe fn scroll_to(&mut self, y: f32) {
        let (_, _, vh) = self.doc_geo();
        let (_, _, _) = self.doc_geo();
        let s = self.scale;
        let sheet_h = vh;
        let maxs = self.doc.as_ref().map(|d| (d.height + 34.0 * s + 24.0 * s - sheet_h).max(0.0)).unwrap_or(0.0);
        self.scroll_t = (y - 46.0 * s).clamp(0.0, maxs);
        SetTimer(Some(self.hwnd), 1, 8, None);
    }
}

fn fs_marker(it: &Item) -> f32 {
    match &it.kind {
        ItemKind::Text(t) => t.size,
        _ => 16.0,
    }
}

fn continue_none() -> (String, PathBuf, String) {
    ("Scheda".to_string(), PathBuf::new(), String::new())
}

impl Doc {
    pub(crate) fn code_text(&self, i: usize) -> Option<String> {
        match self.items.get(i).map(|it| &it.kind) {
            Some(ItemKind::Code(c)) => Some(c.src.clone()),
            _ => None,
        }
    }
    pub(crate) fn copy_hit(&self, x: f32, y: f32) -> Option<usize> {
        self.copy_hits.iter().find(|(_, r)| r.hit(x, y)).map(|(i, _)| *i)
    }
}


