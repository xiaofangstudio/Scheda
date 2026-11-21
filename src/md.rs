//! Markdown → 渲染块模型（GFM：表格 / 任务列表 / 删除线 / 脚注标记）。

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: bool,
}

#[derive(Clone, Default, Debug)]
pub struct Run {
    pub text: String,
    pub s: Style,
    pub href: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TokKind {
    Plain,
    Keyword,
    Str,
    Comment,
    Num,
    Ty,
    Punct,
}

#[derive(Clone, Debug)]
pub struct Tok {
    pub t: String,
    pub k: TokKind,
}

#[derive(Clone, Debug)]
pub struct ListItem {
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug)]
pub enum Block {
    Heading { level: u8, runs: Vec<Run> },
    Para { runs: Vec<Run> },
    Code { lang: String, lines: Vec<Vec<Tok>> },
    Quote { blocks: Vec<Block> },
    List { ordered: bool, start: u64, items: Vec<ListItem> },
    Table { aligns: Vec<Align>, head: Vec<Vec<Run>>, rows: Vec<Vec<Vec<Run>>> },
    Rule,
    Image { src: String, alt: String },
}

#[derive(Default)]
struct Builder {
    stack: Vec<Ctx>,
    runs: Vec<Run>,
    style: Vec<Style>,
    heading: Option<u8>,
    href: Option<String>,
    img: Option<String>,
    table_aligns: Vec<Align>,
}

enum Ctx {
    Blocks(Vec<Block>),
    Item { task: Option<bool>, blocks: Vec<Block> },
    List { ordered: bool, start: u64, items: Vec<ListItem> },
    Code { lang: String, text: String },
    Table { head: Vec<Vec<Run>>, rows: Vec<Vec<Vec<Run>>>, row: Vec<Vec<Run>>, in_head: bool },
}

impl Builder {
    fn cur_style(&self) -> Style {
        self.style.last().copied().unwrap_or_default()
    }
    fn push_block(&mut self, b: Block) {
        match self.stack.last_mut() {
            Some(Ctx::Blocks(v)) => v.push(b),
            Some(Ctx::Item { blocks, .. }) => blocks.push(b),
            Some(Ctx::List { items, .. }) => {
                // 没有 Item 包裹的块，兜底塞进最后一个条目
                if let Some(it) = items.last_mut() {
                    it.blocks.push(b);
                }
            }
            _ => {}
        }
    }
    fn take_blocks(&mut self) -> Vec<Block> {
        match self.stack.last_mut() {
            Some(Ctx::Blocks(v)) => std::mem::take(v),
            Some(Ctx::Item { blocks, .. }) => std::mem::take(blocks),
            _ => Vec::new(),
        }
    }
    fn flush_para(&mut self) {
        if !self.runs.is_empty() {
            let runs = std::mem::take(&mut self.runs);
            self.push_block(Block::Para { runs });
        }
    }
    fn add_text(&mut self, t: &str, extra: Style, href: Option<String>) {
        if t.is_empty() {
            return;
        }
        let mut s = self.cur_style();
        s.bold |= extra.bold;
        s.italic |= extra.italic;
        s.strike |= extra.strike;
        s.code |= extra.code;
        s.link |= extra.link;
        let h = href.or_else(|| self.href.clone());
        if let Some(last) = self.runs.last_mut() {
            if last.s == s && last.href == h {
                last.text.push_str(t);
                return;
            }
        }
        self.runs.push(Run { text: t.to_string(), s, href: h });
    }
}

fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

pub fn parse(src: &str) -> Vec<Block> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);

    let mut b = Builder::default();
    b.stack.push(Ctx::Blocks(Vec::new()));

    for ev in Parser::new_ext(src, opts) {
        match ev {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    b.flush_para();
                    b.heading = Some(level_to_u8(level));
                }
                Tag::Paragraph => {}
                Tag::BlockQuote(_) => {
                    b.flush_para();
                    b.stack.push(Ctx::Blocks(Vec::new()));
                }
                Tag::List(start) => {
                    b.flush_para();
                    b.stack.push(Ctx::List {
                        ordered: start.is_some(),
                        start: start.unwrap_or(1),
                        items: Vec::new(),
                    });
                }
                Tag::Item => {
                    b.flush_para();
                    b.stack.push(Ctx::Item { task: None, blocks: Vec::new() });
                }
                Tag::CodeBlock(kind) => {
                    b.flush_para();
                    let lang = match kind {
                        CodeBlockKind::Fenced(s) => s.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    b.stack.push(Ctx::Code { lang, text: String::new() });
                }
                Tag::Table(aligns) => {
                    b.flush_para();
                    b.table_aligns = aligns
                        .iter()
                        .map(|a| match a {
                            pulldown_cmark::Alignment::Left => Align::Left,
                            pulldown_cmark::Alignment::Center => Align::Center,
                            pulldown_cmark::Alignment::Right => Align::Right,
                            pulldown_cmark::Alignment::None => Align::None,
                        })
                        .collect();
                    b.stack.push(Ctx::Table { head: Vec::new(), rows: Vec::new(), row: Vec::new(), in_head: false });
                }
                Tag::TableHead => {
                    b.runs.clear();
                    if let Some(Ctx::Table { row, in_head, .. }) = b.stack.last_mut() {
                        row.clear();
                        *in_head = true;
                    }
                }
                Tag::TableRow => {
                    b.runs.clear();
                    if let Some(Ctx::Table { row, in_head, .. }) = b.stack.last_mut() {
                        row.clear();
                        *in_head = false;
                    }
                }
                Tag::TableCell => {
                    b.runs.clear();
                }
                Tag::Emphasis => {
                    let mut s = b.cur_style();
                    s.italic = true;
                    b.style.push(s);
                }
                Tag::Strong => {
                    let mut s = b.cur_style();
                    s.bold = true;
                    b.style.push(s);
                }
                Tag::Strikethrough => {
                    let mut s = b.cur_style();
                    s.strike = true;
                    b.style.push(s);
                }
                Tag::Link { dest_url, .. } => {
                    let mut s = b.cur_style();
                    s.link = true;
                    b.style.push(s);
                    b.href = Some(dest_url.to_string());
                }
                Tag::Image { dest_url, .. } => {
                    b.img = Some(dest_url.to_string());
                }
                Tag::FootnoteDefinition(name) => {
                    b.flush_para();
                    b.stack.push(Ctx::Blocks(Vec::new()));
                    let _ = name;
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Heading(_) => {
                    let level = b.heading.take().unwrap_or(1);
                    if !b.runs.is_empty() {
                        let runs = std::mem::take(&mut b.runs);
                        b.push_block(Block::Heading { level, runs });
                    }
                }
                TagEnd::Paragraph => b.flush_para(),
                TagEnd::BlockQuote(_) => {
                    b.flush_para();
                    let blocks = b.take_blocks();
                    if b.stack.len() > 1 {
                        b.stack.pop();
                    }
                    b.push_block(Block::Quote { blocks });
                }
                TagEnd::List(_) => {
                    b.flush_para();
                    if let Some(Ctx::List { ordered, start, items }) = b.stack.pop() {
                        b.push_block(Block::List { ordered, start, items });
                    }
                }
                TagEnd::Item => {
                    b.flush_para();
                    if let Some(Ctx::Item { task, blocks }) = b.stack.pop() {
                        if let Some(Ctx::List { items, .. }) = b.stack.last_mut() {
                            items.push(ListItem { task, blocks });
                        }
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some(Ctx::Code { lang, text }) = b.stack.pop() {
                        let lines = highlight(&lang, &text);
                        b.push_block(Block::Code { lang, lines });
                    }
                }
                TagEnd::Table => {
                    if let Some(Ctx::Table { head, rows, .. }) = b.stack.pop() {
                        let aligns = std::mem::take(&mut b.table_aligns);
                        b.push_block(Block::Table { aligns, head, rows });
                    }
                }
                TagEnd::TableHead => {
                    b.flush_para();
                    if let Some(Ctx::Table { head, row, .. }) = b.stack.last_mut() {
                        *head = std::mem::take(row);
                    }
                    b.runs.clear();
                }
                TagEnd::TableRow => {
                    if let Some(Ctx::Table { rows, row, in_head, .. }) = b.stack.last_mut() {
                        if !*in_head {
                            rows.push(std::mem::take(row));
                        }
                    }
                }
                TagEnd::TableCell => {
                    let cell = std::mem::take(&mut b.runs);
                    if let Some(Ctx::Table { row, .. }) = b.stack.last_mut() {
                        row.push(cell);
                    }
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    // 栈底没有哨兵元素，收尾时只要栈非空就弹一层；
                    // 否则粗体会从这一处一路粘到文末（曾经正文整体发粗就栽在这）。
                    let _ = b.style.pop();
                }
                TagEnd::Link => {
                    let _ = b.style.pop();
                    b.href = None;
                }
                TagEnd::Image => {
                    if let Some(src) = b.img.take() {
                        let alt = std::mem::take(&mut b.runs).into_iter().map(|r| r.text).collect::<String>();
                        if alt.is_empty() {
                            b.push_block(Block::Image { src, alt: String::new() });
                        } else {
                            // 有替代文本的图片，作为独立块渲染
                            b.push_block(Block::Image { src, alt });
                        }
                    }
                }
                TagEnd::FootnoteDefinition => {
                    b.flush_para();
                    let _ = b.take_blocks();
                    if b.stack.len() > 1 {
                        b.stack.pop();
                    }
                }
                _ => {}
            },
            Event::Text(t) => {
                let in_code = matches!(b.stack.last(), Some(Ctx::Code { .. }));
                if in_code {
                    if let Some(Ctx::Code { text, .. }) = b.stack.last_mut() {
                        text.push_str(&t);
                    }
                } else {
                    b.add_text(&t, Style::default(), None);
                }
            }
            Event::Code(c) => {
                let mut s = Style::default();
                s.code = true;
                b.add_text(&c, s, None);
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                let text = strip_html(&h);
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    b.add_text(trimmed, Style::default(), None);
                }
            }
            Event::SoftBreak => {
                if let Some(Ctx::Code { text, .. }) = b.stack.last_mut() {
                    text.push('\n');
                } else {
                    b.add_text(" ", Style::default(), None);
                }
            }
            Event::HardBreak => {
                if let Some(Ctx::Code { text, .. }) = b.stack.last_mut() {
                    text.push('\n');
                } else {
                    b.add_text("\n", Style::default(), None);
                }
            }
            Event::Rule => {
                b.flush_para();
                b.push_block(Block::Rule);
            }
            Event::TaskListMarker(checked) => {
                if let Some(Ctx::Item { task, .. }) = b.stack.last_mut() {
                    *task = Some(checked);
                }
            }
            Event::FootnoteReference(name) => {
                let mut s = Style::default();
                s.code = true;
                b.add_text(&format!("[{}]", name), s, None);
            }
            _ => {}
        }
    }
    b.flush_para();
    let blocks = b.take_blocks();
    blocks
}

fn level_to_u8(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        _ => 6,
    }
}

/// 取渲染后的纯文本（供 Ctrl+C 复制、TOC、窗口标题使用）
pub fn plain_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    collect(blocks, &mut out);
    out
}

fn collect(blocks: &[Block], out: &mut String) {
    for b in blocks {
        match b {
            Block::Heading { runs, .. } | Block::Para { runs } => {
                for r in runs {
                    out.push_str(&r.text);
                }
                out.push('\n');
            }
            Block::Code { lines, .. } => {
                for l in lines {
                    for t in l {
                        out.push_str(&t.t);
                    }
                    out.push('\n');
                }
            }
            Block::Quote { blocks } => collect(blocks, out),
            Block::List { items, .. } => {
                for it in items {
                    collect(&it.blocks, out);
                }
            }
            Block::Table { head, rows, .. } => {
                for c in head {
                    for r in c {
                        out.push_str(&r.text);
                    }
                    out.push('\t');
                }
                out.push('\n');
                for row in rows {
                    for c in row {
                        for r in c {
                            out.push_str(&r.text);
                        }
                        out.push('\t');
                    }
                    out.push('\n');
                }
            }
            Block::Image { alt, .. } => {
                out.push_str(alt);
                out.push('\n');
            }
            Block::Rule => out.push_str("—\n"),
        }
    }
}

/// 把标题抽出来做目录
pub fn headings(blocks: &[Block]) -> Vec<(u8, String)> {
    let mut v = Vec::new();
    fn walk(blocks: &[Block], v: &mut Vec<(u8, String)>) {
        for b in blocks {
            match b {
                Block::Heading { level, runs } => {
                    let t: String = runs.iter().map(|r| r.text.clone()).collect();
                    if !t.trim().is_empty() {
                        v.push((*level, t));
                    }
                }
                Block::Quote { blocks } => walk(blocks, v),
                Block::List { items, .. } => {
                    for it in items {
                        walk(&it.blocks, v);
                    }
                }
                _ => {}
            }
        }
    }
    walk(blocks, &mut v);
    v
}

// ————————————————————————————————————————————————————————————————
// 语法高亮（轻量、按语言给关键字，够用且不拖慢启动）
// ————————————————————————————————————————————————————————————————

struct Lang {
    line_comment: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
    keywords: &'static [&'static str],
}

fn lang_of(name: &str) -> Lang {
    let n = name.trim().to_ascii_lowercase();
    let (kw, line, quotes): (&'static [&'static str], &'static [&'static str], &'static [char]) = match n.as_str() {
        "rust" | "rs" => (
            &["fn","let","mut","pub","use","mod","impl","trait","struct","enum","match","if","else","for","while","loop","return","break","continue","const","static","unsafe","where","as","in","ref","move","dyn","async","await","crate","self","super","type","impl","extern","true","false"],
            &["//"],
            &['"'],
        ),
        "js" | "javascript" | "jsx" | "ts" | "typescript" | "tsx" => (
            &["function","const","let","var","if","else","for","while","do","return","class","extends","new","this","import","export","from","default","async","await","try","catch","finally","throw","typeof","instanceof","null","undefined","true","false","switch","case","break","continue","interface","type","enum","public","private","readonly","static","yield","delete","void","super"],
            &["//"],
            &['"', '\'', '`'],
        ),
        "py" | "python" => (
            &["def","class","if","elif","else","for","while","return","import","from","as","with","try","except","finally","raise","lambda","pass","break","continue","global","nonlocal","yield","assert","del","in","is","not","and","or","None","True","False","async","await","self"],
            &["#"],
            &['"', '\''],
        ),
        "go" | "golang" => (
            &["func","package","import","var","const","type","struct","interface","map","chan","go","defer","select","case","switch","if","else","for","range","return","break","continue","fallthrough","nil","true","false","make","new"],
            &["//"],
            &['"', '`'],
        ),
        "c" | "h" | "cpp" | "c++" | "cc" | "hpp" => (
            &["int","char","float","double","void","long","short","unsigned","signed","const","static","struct","class","enum","union","typedef","if","else","for","while","do","switch","case","return","break","continue","sizeof","new","delete","public","private","protected","virtual","template","namespace","using","auto","true","false","nullptr","include","define"],
            &["//"],
            &['"', '\''],
        ),
        "java" | "kt" | "kotlin" => (
            &["public","private","protected","class","interface","extends","implements","static","final","void","int","long","double","float","boolean","char","String","if","else","for","while","do","switch","case","return","break","continue","new","this","super","try","catch","finally","throw","throws","import","package","fun","val","var","object","when","null","true","false"],
            &["//"],
            &['"', '\''],
        ),
        "sh" | "bash" | "shell" | "zsh" | "ps1" | "powershell" => (
            &["if","then","else","elif","fi","for","do","done","while","case","esac","function","return","export","local","source","echo","cd","sudo","set","unset","read","in"],
            &["#"],
            &['"', '\''],
        ),
        "json" => (&["true","false","null"], &[], &['"']),
        "toml" | "ini" | "conf" => (&["true","false"], &["#"], &['"', '\'']),
        "yaml" | "yml" => (&["true","false","null","yes","no"], &["#"], &['"', '\'']),
        "sql" => (
            &["select","from","where","join","left","right","inner","outer","on","group","by","order","having","insert","into","values","update","set","delete","create","table","index","view","drop","alter","as","and","or","not","null","limit","offset","distinct","union","all","case","when","then","else","end","primary","key","foreign","references","default","if","exists"],
            &["--"],
            &['"', '\''],
        ),
        "html" | "xml" | "svg" | "vue" => (&["div","span","class","id","src","href","style"], &[], &['"', '\'']),
        "css" | "scss" | "less" => (&["color","background","margin","padding","display","flex","grid","position","width","height","border","radius","font","transition","transform"], &[], &['"', '\'']),
        "lua" => (
            &["local","function","end","if","then","else","elseif","for","while","do","return","break","nil","true","false","and","or","not","in"],
            &["--"],
            &['"', '\''],
        ),
        _ => (&[], &[], &['"', '\'']),
    };
    Lang {
        line_comment: line,
        block_comment: if matches!(n.as_str(), "rust" | "rs" | "js" | "javascript" | "jsx" | "ts" | "typescript" | "tsx" | "c" | "h" | "cpp" | "c++" | "cc" | "hpp" | "java" | "kt" | "kotlin" | "go" | "golang" | "css" | "scss" | "less") {
            Some(("/*", "*/"))
        } else {
            None
        },
        quotes,
        keywords: kw,
    }
}

fn is_kw(kw: &[&str], w: &str) -> bool {
    kw.iter().any(|k| *k == w)
}

pub fn highlight(lang_name: &str, src: &str) -> Vec<Vec<Tok>> {
    let lang = lang_of(lang_name);
    let mut out: Vec<Vec<Tok>> = Vec::new();
    let mut line: Vec<Tok> = Vec::new();
    let mut in_block_comment = false;
    let mut string_open: Option<char> = None;

    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut plain = String::new();

    fn flush(line: &mut Vec<Tok>, plain: &mut String) {
        if !plain.is_empty() {
            line.push(Tok { t: std::mem::take(plain), k: TokKind::Plain });
        }
    }

    while i < chars.len() {
        let c = chars[i];

        if in_block_comment {
            let mut s = String::new();
            while i < chars.len() {
                if let Some((_, end)) = lang.block_comment {
                    if c == end.chars().next().unwrap_or('*') && i + 1 < chars.len() && chars[i + 1] == end.chars().nth(1).unwrap_or('/') {
                        s.push(c);
                        s.push(chars[i + 1]);
                        i += 2;
                        break;
                    }
                }
                s.push(chars[i]);
                if chars[i] == '\n' {
                    i += 1;
                    flush(&mut line, &mut plain);
                    line.push(Tok { t: s.clone(), k: TokKind::Comment });
                    out.push(std::mem::take(&mut line));
                    s.clear();
                } else {
                    i += 1;
                }
            }
            if !s.is_empty() {
                line.push(Tok { t: s, k: TokKind::Comment });
            }
            in_block_comment = false;
            continue;
        }

        if let Some(q) = string_open {
            let mut s = String::new();
            while i < chars.len() {
                let ch = chars[i];
                s.push(ch);
                i += 1;
                if ch == '\\' && i < chars.len() {
                    s.push(chars[i]);
                    i += 1;
                    continue;
                }
                if ch == q {
                    string_open = None;
                    break;
                }
                if ch == '\n' {
                    string_open = None;
                    break;
                }
            }
            line.push(Tok { t: s, k: TokKind::Str });
            continue;
        }

        // 行注释
        let mut matched_comment = false;
        for lc in lang.line_comment {
            let l: Vec<char> = lc.chars().collect();
            if i + l.len() <= chars.len() && chars[i..i + l.len()] == l[..] {
                let mut s = String::new();
                while i < chars.len() && chars[i] != '\n' {
                    s.push(chars[i]);
                    i += 1;
                }
                flush(&mut line, &mut plain);
                line.push(Tok { t: s, k: TokKind::Comment });
                matched_comment = true;
                break;
            }
        }
        if matched_comment {
            continue;
        }

        // 块注释开始
        if let Some((start, end)) = lang.block_comment {
            let s: Vec<char> = start.chars().collect();
            if i + s.len() <= chars.len() && chars[i..i + s.len()] == s[..] {
                flush(&mut line, &mut plain);
                let mut txt = String::new();
                let mut closed = false;
                while i < chars.len() {
                    txt.push(chars[i]);
                    let e: Vec<char> = end.chars().collect();
                    if i + e.len() <= chars.len() && chars[i..i + e.len()] == e[..] {
                        for _ in 0..e.len() {
                            if i + 1 < chars.len() {
                                txt.push(chars[i + 1]);
                                i += 1;
                            }
                        }
                        i += 1;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                line.push(Tok { t: txt, k: TokKind::Comment });
                in_block_comment = !closed;
                continue;
            }
        }

        // 字符串
        if lang.quotes.contains(&c) {
            string_open = Some(c);
            i += 1;
            let mut s = String::new();
            s.push(c);
            while i < chars.len() {
                let ch = chars[i];
                s.push(ch);
                i += 1;
                if ch == '\\' && i < chars.len() {
                    s.push(chars[i]);
                    i += 1;
                    continue;
                }
                if ch == c {
                    string_open = None;
                    break;
                }
                if ch == '\n' {
                    string_open = None;
                    break;
                }
            }
            flush(&mut line, &mut plain);
            line.push(Tok { t: s, k: TokKind::Str });
            continue;
        }

        // 数字
        if c.is_ascii_digit() {
            let mut s = String::new();
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '_') {
                s.push(chars[i]);
                i += 1;
            }
            flush(&mut line, &mut plain);
            line.push(Tok { t: s, k: TokKind::Num });
            continue;
        }

        // 标识符 / 关键字
        if c.is_alphabetic() || c == '_' || c == '$' {
            let mut s = String::new();
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                s.push(chars[i]);
                i += 1;
            }
            flush(&mut line, &mut plain);
            let k = if is_kw(lang.keywords, &s) {
                TokKind::Keyword
            } else if s.chars().next().map(|x| x.is_uppercase()).unwrap_or(false) {
                TokKind::Ty
            } else {
                TokKind::Plain
            };
            line.push(Tok { t: s, k });
            continue;
        }

        if c == '\n' {
            flush(&mut line, &mut plain);
            out.push(std::mem::take(&mut line));
            i += 1;
            continue;
        }

        plain.push(c);
        i += 1;
    }
    flush(&mut line, &mut plain);
    if !line.is_empty() || !out.is_empty() {
        out.push(line);
    }
    // 去掉末尾多余空行
    while out.last().map(|l| l.is_empty()).unwrap_or(false) {
        out.pop();
    }
    out
}
