//! エディタ用のMarkdownシンタックスハイライトと、カーソル位置まわりの補助関数。

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Color32, FontFamily, FontId, Stroke, Vec2};
use std::ops::Range;
use robsidian_core::{parse, LinkKind};

#[derive(Clone, Copy, PartialEq)]
pub struct Colors {
    pub text: Color32,
    pub dim: Color32,
    pub heading: Color32,
    pub accent: Color32,
    pub tag: Color32,
    pub strong: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub quote: Color32,
    pub mark_bg: Color32,
}

impl Colors {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                text: Color32::from_rgb(0xd4, 0xd7, 0xdd),
                dim: Color32::from_rgb(0x74, 0x7a, 0x86),
                heading: Color32::from_rgb(0xf2, 0xf3, 0xf5),
                accent: Color32::from_rgb(0x6c, 0xb6, 0xff),
                tag: Color32::from_rgb(0x5f, 0xd3, 0xa6),
                strong: Color32::from_rgb(0xff, 0xd4, 0x79),
                code: Color32::from_rgb(0xe6, 0xa5, 0x7e),
                code_bg: Color32::from_rgb(0x26, 0x29, 0x30),
                quote: Color32::from_rgb(0xa6, 0xab, 0xb5),
                mark_bg: Color32::from_rgb(0x5c, 0x4d, 0x12),
            }
        } else {
            Self {
                text: Color32::from_rgb(0x2a, 0x2d, 0x33),
                dim: Color32::from_rgb(0x8a, 0x8f, 0x99),
                heading: Color32::from_rgb(0x10, 0x12, 0x16),
                accent: Color32::from_rgb(0x1a, 0x6f, 0xd6),
                tag: Color32::from_rgb(0x0f, 0x8a, 0x5f),
                strong: Color32::from_rgb(0xa8, 0x5d, 0x00),
                code: Color32::from_rgb(0xb0, 0x4a, 0x1e),
                code_bg: Color32::from_rgb(0xee, 0xef, 0xf2),
                quote: Color32::from_rgb(0x5c, 0x61, 0x6b),
                mark_bg: Color32::from_rgb(0xff, 0xec, 0x99),
            }
        }
    }
}

/// 記法文字を畳むときのフォントサイズ。0にはできない（幅0・高さ0の文字として残す）。
const HIDDEN_SIZE: f32 = 0.01;

/// 本文の行の高さ（文字サイズに対する倍率）。
const LINE_HEIGHT: f32 = 1.5;

/// 文字の上（または背面）に重ねて描く装飾。範囲は**文字数**での位置。
#[derive(Debug, Clone, PartialEq)]
pub struct Deco {
    pub kind: DecoKind,
    pub chars: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecoKind {
    /// 箇条書きの「・」
    Bullet,
    /// タスクのチェックボックス（範囲は `[ ]` の3文字）
    Checkbox(bool),
    /// 引用の縦線（範囲は行全体）
    Quote,
    /// 水平線（範囲は行全体）
    Rule,
    /// コードブロックの背景（範囲は行全体）
    Code,
    /// 画像（範囲は行全体、中身はリンク先）
    Image(String),
    /// 表（範囲は表全体。先頭の行が見出し）
    Table(Vec<TableRow>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    /// この行の文字範囲（行の高さと位置を得るため）
    pub chars: Range<usize>,
    pub cells: Vec<String>,
}

pub struct Options<'a> {
    pub size: f32,
    pub colors: &'a Colors,
    /// Live Preview: カーソルの無い行の記法を畳む
    pub live: bool,
    /// 記法をそのまま見せる行の範囲（カーソルや選択のある行。0始まり、両端含む）
    pub reveal: Option<(usize, usize)>,
    /// 画像リンク先 → 表示サイズ。読み込めない・未読み込みなら None
    pub image: &'a dyn Fn(&str) -> Option<Vec2>,
}

#[derive(Default, Clone)]
pub struct Layout {
    pub job: LayoutJob,
    pub decos: Vec<Deco>,
}

struct Painter<'a> {
    job: LayoutJob,
    decos: Vec<Deco>,
    /// これまでに積んだ文字数
    chars: usize,
    size: f32,
    c: &'a Colors,
}

impl Painter<'_> {
    fn fmt(&self, color: Color32) -> TextFormat {
        TextFormat {
            font_id: FontId::new(self.size, FontFamily::Proportional),
            line_height: Some(self.size * LINE_HEIGHT),
            color,
            ..Default::default()
        }
    }

    fn mono(&self, color: Color32) -> TextFormat {
        TextFormat {
            font_id: FontId::new(self.size * 0.93, FontFamily::Monospace),
            color,
            ..Default::default()
        }
    }

    /// 記法文字の書式。畳むときは極小・透明、見せるときは薄い色。
    fn mark(&self, hide: bool) -> TextFormat {
        if hide {
            TextFormat {
                font_id: FontId::new(HIDDEN_SIZE, FontFamily::Proportional),
                color: Color32::TRANSPARENT,
                ..Default::default()
            }
        } else {
            self.fmt(self.c.dim)
        }
    }

    /// 幅は保ったまま見えなくする（上に装飾を描く場所として使う）。
    fn ghost(&self, hide: bool) -> TextFormat {
        self.fmt(if hide { Color32::TRANSPARENT } else { self.c.dim })
    }

    fn push(&mut self, text: &str, format: TextFormat) -> Range<usize> {
        let start = self.chars;
        if !text.is_empty() {
            self.job.append(text, 0.0, format);
            self.chars += text.chars().count();
        }
        start..self.chars
    }

    fn deco(&mut self, kind: DecoKind, chars: Range<usize>) {
        self.decos.push(Deco { kind, chars });
    }

    /// 1行ぶんのインライン要素（コード・リンク・強調・タグ）を積む。
    fn inline(&mut self, line: &str, hide: bool, base: Color32) {
        let b = line.as_bytes();
        let mut plain_from = 0;
        let mut i = 0;
        // (記法の開始, 中身の開始, 中身の終了, 記法の終了, 中身の書式)
        type Token = (usize, usize, usize, usize, TextFormat);
        while i < b.len() {
            let token: Option<Token> = match b[i] {
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'`' => {
                    let n = b[i..].iter().take_while(|&&c| c == b'`').count();
                    line[i + n..].find(&line[i..i + n]).map(|close| {
                        let mut f = self.mono(self.c.code);
                        f.background = self.c.code_bg;
                        f.line_height = Some(self.size * LINE_HEIGHT);
                        (i, i + n, i + n + close, i + n + close + n, f)
                    })
                }
                b'!' | b'[' => {
                    let bang = usize::from(b[i] == b'!');
                    let open = i + bang;
                    let f = self.fmt(self.c.accent);
                    if line[open..].starts_with("[[") {
                        line[open + 2..].find("]]").and_then(|close| {
                            let inner = &line[open + 2..open + 2 + close];
                            if inner.is_empty() || inner.contains('[') {
                                return None;
                            }
                            // `[[リンク先|表示名]]` は表示名だけを見せる
                            let shown = inner.rfind('|').map_or(0, |p| p + 1);
                            let end = open + 2 + close;
                            Some((i, open + 2 + shown, end, end + 2, f))
                        })
                    } else if line[open..].starts_with('[') {
                        line[open..].find("](").and_then(|mid| {
                            if mid <= 1 || line[open + 1..open + mid].contains('[') {
                                return None;
                            }
                            let close = line[open + mid..].find(')')?;
                            Some((i, open + 1, open + mid, open + mid + close + 1, f))
                        })
                    } else {
                        None
                    }
                }
                b'*' | b'_' | b'~' | b'=' => {
                    let ch = b[i];
                    let double = b.get(i + 1) == Some(&ch);
                    let n = if double { 2 } else { 1 };
                    let word = |p: Option<&u8>| p.is_some_and(|c| c.is_ascii_alphanumeric());
                    let ok = match (ch, double) {
                        (b'*', _) => true,
                        // snake_case を斜体にしないよう、`_` は語の境界でだけ有効
                        (b'_', false) => !(i > 0 && word(b.get(i - 1))),
                        (b'~' | b'=', true) => true,
                        _ => false,
                    };
                    let opens = b.get(i + n).is_some_and(|c| !c.is_ascii_whitespace() && *c != ch);
                    if ok && opens {
                        let marker = &line[i..i + n];
                        line[i + n..]
                            .match_indices(marker)
                            .map(|(p, _)| i + n + p)
                            .find(|&p| {
                                !b[p - 1].is_ascii_whitespace()
                                    && b.get(p + n) != Some(&ch)
                                    && (ch != b'_' || !word(b.get(p + n)))
                            })
                            .map(|close| {
                                let mut f = self.fmt(base);
                                match (ch, double) {
                                    (b'*', true) => f.color = self.c.strong,
                                    (b'~', _) => {
                                        f.strikethrough = Stroke::new(1.0, self.c.dim);
                                        f.color = self.c.dim;
                                    }
                                    (b'=', _) => f.background = self.c.mark_bg,
                                    _ => f.italics = true,
                                }
                                (i, i + n, close, close + n, f)
                            })
                    } else {
                        None
                    }
                }
                b'#' if i == 0 || b[i - 1].is_ascii_whitespace() => {
                    let rest = &line[i + 1..];
                    let end = rest
                        .char_indices()
                        .find(|&(_, c)| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '/')))
                        .map_or(rest.len(), |(p, _)| p);
                    (end > 0 && !rest[..end].chars().all(|c| c.is_ascii_digit()))
                        .then(|| (i, i, i + 1 + end, i + 1 + end, self.fmt(self.c.tag)))
                }
                _ => None,
            };
            match token {
                Some((start, inner_start, inner_end, end, format)) => {
                    self.push(&line[plain_from..start], self.fmt(base));
                    self.push(&line[start..inner_start], self.mark(hide));
                    self.push(&line[inner_start..inner_end], format);
                    self.push(&line[inner_end..end], self.mark(hide));
                    plain_from = end;
                    i = end;
                }
                None => i += 1,
            }
        }
        self.push(&line[plain_from..], self.fmt(base));
    }

    /// 行頭の記号（リスト・タスク・引用）を積み、残りの本文の開始位置と文字色を返す。
    fn block_prefix(&mut self, line: &str, hide: bool) -> (usize, Color32) {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let text = self.c.text;
        if trimmed.starts_with("> ") || trimmed == ">" {
            self.push(&line[..indent], self.fmt(text));
            let end = (indent + 2).min(line.len());
            self.push(&line[indent..end], self.ghost(hide));
            return (end, self.c.quote);
        }
        let bullet = ["- ", "* ", "+ "].iter().any(|m| trimmed.starts_with(m));
        if bullet {
            self.push(&line[..indent], self.fmt(text));
            let rest = &trimmed[2..];
            let checked = rest.starts_with("[x] ") || rest.starts_with("[X] ");
            if checked || rest.starts_with("[ ] ") {
                // `- [ ] ` → 先頭の `- ` を畳み、`[ ]` の位置にチェックボックスを描く
                self.push(&line[indent..indent + 2], self.mark(hide));
                let range = self.push(&line[indent + 2..indent + 5], self.ghost(hide));
                if hide {
                    self.deco(DecoKind::Checkbox(checked), range);
                }
                self.push(" ", self.fmt(text));
                return (indent + 6, if checked { self.c.dim } else { text });
            }
            let range = self.push(&line[indent..indent + 1], self.ghost(hide));
            if hide {
                self.deco(DecoKind::Bullet, range);
            }
            self.push(" ", self.fmt(text));
            return (indent + 2, text);
        }
        let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && trimmed[digits..].starts_with(". ") {
            let end = indent + digits + 2;
            self.push(&line[..end], self.fmt(self.c.dim));
            return (end, text);
        }
        (0, text)
    }
}

/// 行が画像の埋め込みだけで出来ていれば、そのリンク先を返す。
fn image_only(line: &str) -> Option<String> {
    let t = line.trim();
    if let Some(inner) = t.strip_prefix("![[").and_then(|r| r.strip_suffix("]]")) {
        let target = inner.split(['|', '#']).next()?.trim();
        return (!target.is_empty() && !inner.contains("]]")).then(|| target.to_string());
    }
    let rest = t.strip_prefix("![")?.strip_suffix(')')?;
    let (alt, url) = rest.split_once("](")?;
    if alt.contains(']') || url.contains(')') || url.contains("://") {
        return None;
    }
    let url = url.trim().trim_start_matches('<').trim_end_matches('>');
    Some(parse::percent_decode(url))
}

/// `| a | b |` をセルに分ける（`\|` は区切りとして扱わない）。
fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').filter(|r| !r.ends_with('\\')).unwrap_or(t);
    let mut cells = vec![String::new()];
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                chars.next();
                cells.last_mut().expect("never empty").push('|');
            }
            '|' => cells.push(String::new()),
            c => cells.last_mut().expect("never empty").push(c),
        }
    }
    cells.into_iter().map(|c| c.trim().to_string()).collect()
}

/// 表の2行目（`|---|:--:|`）か。
fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|')
        && t.contains('-')
        && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

/// 表のセルなど、エディタ外に描く短い文字列をインライン記法つきで組む。
pub fn inline_job(text: &str, size: f32, colors: &Colors, color: Color32) -> LayoutJob {
    let mut p = Painter { job: LayoutJob::default(), decos: Vec::new(), chars: 0, size, c: colors };
    p.inline(text, true, color);
    p.job
}

fn is_rule(trimmed: &str) -> bool {
    let mut chars = trimmed.chars().filter(|c| !c.is_whitespace());
    let Some(first) = chars.next() else { return false };
    matches!(first, '-' | '*' | '_')
        && chars.clone().all(|c| c == first)
        && chars.count() >= 2
}

pub fn layout(text: &str, opt: &Options) -> Layout {
    let mut p = Painter {
        job: LayoutJob::default(),
        decos: Vec::new(),
        chars: 0,
        size: opt.size,
        c: opt.colors,
    };
    let size = opt.size;
    let mut in_frontmatter = text.starts_with("---\n") || text.starts_with("---\r\n");
    let mut fence: Option<(u8, usize)> = None;
    // 処理中の表: (最後の行, 整形表示するか, 表の開始文字位置, 集めた行)
    let mut table: Option<(usize, bool, usize, Vec<TableRow>)> = None;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    fn content(raw: &str) -> &str {
        raw.trim_end_matches(['\n', '\r'])
    }

    for (n, raw) in lines.iter().copied().enumerate() {
        let line = content(raw);
        let eol = &raw[line.len()..];
        let trimmed = line.trim_start();
        let revealed = opt.reveal.is_some_and(|(a, b)| (a..=b).contains(&n));
        let hide = opt.live && !revealed;

        if in_frontmatter {
            p.push(raw, p.mono(p.c.dim));
            if n > 0 && matches!(line.trim_end(), "---" | "...") {
                in_frontmatter = false;
            }
            continue;
        }

        let marker = {
            let b = trimmed.as_bytes();
            b.first()
                .filter(|&&c| c == b'`' || c == b'~')
                .map(|&c| (c, b.iter().take_while(|&&x| x == c).count()))
                .filter(|&(_, len)| len >= 3)
        };
        let closes = matches!((marker, fence), (Some((c, l)), Some((oc, ol))) if c == oc && l >= ol);
        if fence.is_some() || marker.is_some() {
            let is_fence_line = (fence.is_none() && marker.is_some()) || closes;
            let color = if is_fence_line { p.c.dim } else { p.c.code };
            let start = p.chars;
            if is_fence_line && hide {
                // ``` は畳み、言語名だけ小さく残す（閉じる側の行は高さごと畳まれる）
                let ticks = line.len() - trimmed.len() + marker.map_or(0, |(_, len)| len);
                p.push(&line[..ticks], p.mark(true));
                let mut small = p.mono(p.c.dim);
                small.font_id.size = opt.size * 0.75;
                p.push(&line[ticks..], small);
            } else {
                p.push(line, p.mono(color));
            }
            p.deco(DecoKind::Code, start..p.chars);
            p.push(eol, p.mono(color));
            if fence.is_none() {
                fence = marker;
            } else if closes {
                fence = None;
            }
            continue;
        }

        let level = trimmed.bytes().take_while(|&c| c == b'#').count();
        let is_heading = (1..=6).contains(&level)
            && (trimmed.len() == level || trimmed[level..].starts_with(' '));
        if is_heading {
            let scale = [1.6, 1.4, 1.22, 1.1, 1.05, 1.0][level - 1];
            let mut format = p.fmt(p.c.heading);
            format.font_id.size = size * scale;
            format.line_height = Some(size * scale * LINE_HEIGHT);
            let marker_end = (line.len() - trimmed.len() + level + 1).min(line.len());
            let mut mark = p.mark(hide);
            if !hide {
                mark.font_id.size = size * scale;
                mark.line_height = format.line_height;
            }
            p.push(&line[..marker_end], mark);
            p.push(&line[marker_end..], format);
        } else if is_rule(trimmed) {
            let range = p.push(line, p.ghost(hide));
            if hide {
                p.deco(DecoKind::Rule, range);
            }
        } else if let Some((target, image_size)) = image_only(line)
            .filter(|_| hide)
            .and_then(|t| (opt.image)(&t).map(|s| (t, s)))
        {
            // 行の文字は畳み、行の高さを画像ぶん確保して、そこへ画像を描く
            let mut format = p.mark(true);
            format.line_height = Some(image_size.y + 8.0);
            let range = p.push(line, format);
            p.deco(DecoKind::Image(target), range);
        } else if table.is_some() || trimmed.starts_with('|') {
            if table.is_none() {
                // 表の開始: 2行目が区切り行なら、連続する `|` 行を1つの表として扱う
                let is_table = lines.get(n + 1).is_some_and(|l| is_table_separator(content(l)));
                let end = if is_table {
                    (n + 1..lines.len())
                        .take_while(|&i| content(lines[i]).trim_start().starts_with('|'))
                        .last()
                        .unwrap_or(n)
                } else {
                    n
                };
                // 表の中にカーソルがある間は、表全体をソースのまま見せる
                let touched = opt.reveal.is_some_and(|(a, b)| a <= end && n <= b);
                table = Some((end, is_table && opt.live && !touched, p.chars, Vec::new()));
            }
            let (end, pretty, start, mut rows) = table.take().expect("set above");
            if !pretty {
                p.push(line, p.mono(p.c.text));
            } else if rows.len() == 1 && is_table_separator(line) {
                p.push(line, p.mark(true));
            } else {
                // 文字は透明にして行の高さだけ確保し、上に表を描く
                let mut ghost = p.ghost(true);
                ghost.line_height = Some(size * 1.9);
                let chars = p.push(line, ghost);
                rows.push(TableRow { chars, cells: table_cells(line) });
            }
            if n < end {
                table = Some((end, pretty, start, rows));
            } else if pretty {
                p.deco(DecoKind::Table(rows), start..p.chars);
            }
        } else {
            let start = p.chars;
            let (body, color) = p.block_prefix(line, hide);
            p.inline(&line[body..], hide, color);
            if hide && color == p.c.quote {
                p.deco(DecoKind::Quote, start..p.chars);
            }
        }
        p.push(eol, p.fmt(p.c.text));
    }
    Layout { job: p.job, decos: p.decos }
}

pub fn byte_of_char(text: &str, char_idx: usize) -> usize {
    text.char_indices().nth(char_idx).map_or(text.len(), |(b, _)| b)
}

pub fn char_of_byte(text: &str, byte_idx: usize) -> usize {
    text[..byte_idx.min(text.len())].chars().count()
}

/// `line`（0始まり）の先頭のバイト位置。
pub fn line_start(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    text.match_indices('\n')
        .nth(line - 1)
        .map_or(text.len(), |(i, _)| i + 1)
}

/// カーソル位置にあるリンクのリンク先（`#見出し` 付きの場合はそれを除いたもの）。
pub fn link_at(text: &str, byte_pos: usize) -> Option<String> {
    let start = text[..byte_pos].rfind('\n').map_or(0, |i| i + 1);
    let end = text[byte_pos..].find('\n').map_or(text.len(), |i| byte_pos + i);
    let line = &text[start..end];
    let pos = byte_pos - start;
    parse::parse(line).links.into_iter().find_map(|l| {
        let range = match l.kind {
            LinkKind::Wiki => l.full.clone(),
            LinkKind::Markdown => {
                let open = line[..l.span.start].rfind('[').unwrap_or(l.span.start);
                open..(l.span.end + 1).min(line.len())
            }
        };
        (range.start <= pos && pos <= range.end).then_some(l.target)
    })
}

/// カーソルが `[[入力途中` の中にあれば、その開始バイト位置（`[[` の直後）を返す。
pub fn completion_start(text: &str, cursor: usize) -> Option<usize> {
    let line_start = text[..cursor].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line_start..cursor];
    let open = before.rfind("[[")?;
    let typed = &before[open + 2..];
    if typed.contains(['|', '#', '[', ']']) || typed.len() > 80 {
        return None;
    }
    Some(line_start + open + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        let t = "あ[[Note A|x]]\n見出し [m](b.md) [[Par";
        assert_eq!(link_at(t, 6).as_deref(), Some("Note A"));
        assert_eq!(link_at(t, 0), None);
        let md = t.find("b.md").unwrap();
        assert_eq!(link_at(t, md + 1).as_deref(), Some("b.md"));
        let start = completion_start(t, t.len()).unwrap();
        assert_eq!(&t[start..], "Par");
        assert_eq!(completion_start(t, 3), None);
        assert_eq!(line_start(t, 1), t.find('\n').unwrap() + 1);
        assert_eq!(char_of_byte(t, byte_of_char(t, 5)), 5);
    }

    fn run(text: &str, live: bool, reveal: Option<(usize, usize)>) -> Layout {
        let colors = Colors::new(true);
        let image = |t: &str| (t == "pic.png").then_some(Vec2::new(100.0, 50.0));
        layout(text, &Options { size: 14.0, colors: &colors, live, reveal, image: &image })
    }

    const SAMPLE: &str = "---\na: b\n---\n# H\n- [ ] `c` [[l|別名]] **b** *i* ~~s~~ ==m== #tag [x](y)\n- item\n> quote\n---\n![[pic.png]]\n```\ncode\n```\n| a | b |\n|---|---|\n| 1 | x \\| y |\nsnake_case_name tail";

    #[test]
    fn layout_never_changes_the_text() {
        for (live, reveal) in [(false, None), (true, None), (true, Some((4, 5)))] {
            let out = run(SAMPLE, live, reveal);
            assert_eq!(out.job.text, SAMPLE);
            let total: usize = out.job.sections.iter().map(|s| s.byte_range.end.0 - s.byte_range.start.0).sum();
            assert_eq!(total, SAMPLE.len());
        }
    }

    #[test]
    fn live_hides_marks_and_adds_decorations() {
        let hidden = |out: &Layout| -> String {
            out.job
                .sections
                .iter()
                .filter(|s| s.format.font_id.size == HIDDEN_SIZE)
                .map(|s| &out.job.text[s.byte_range.start.0..s.byte_range.end.0])
                .collect::<Vec<_>>()
                .join("¦")
        };
        let live = run(SAMPLE, true, None);
        assert_eq!(
            hidden(&live),
            "# ¦- ¦`¦`¦[[l|¦]]¦**¦**¦*¦*¦~~¦~~¦==¦==¦[¦](y)¦![[pic.png]]¦```¦```¦|---|---|"
        );
        let slice = |r: &Range<usize>| -> String { SAMPLE.chars().skip(r.start).take(r.len()).collect() };
        let kinds: Vec<_> = live.decos.iter().map(|d| d.kind.clone()).collect();
        assert_eq!(
            kinds[..8],
            [
                DecoKind::Checkbox(false),
                DecoKind::Bullet,
                DecoKind::Quote,
                DecoKind::Rule,
                DecoKind::Image("pic.png".into()),
                DecoKind::Code,
                DecoKind::Code,
                DecoKind::Code,
            ]
        );
        let DecoKind::Table(rows) = &kinds[8] else { panic!("expected a table") };
        assert_eq!(rows.len(), 2);
        assert_eq!((slice(&rows[0].chars).as_str(), &rows[0].cells), ("| a | b |", &vec!["a".to_string(), "b".into()]));
        assert_eq!(rows[1].cells, ["1", "x | y"]);
        assert_eq!(slice(&rows[1].chars), "| 1 | x \\| y |");
        // チェックボックスの範囲は `[ ]` の3文字
        let cb = &live.decos[0].chars;
        let chars: String = SAMPLE.chars().skip(cb.start).take(cb.len()).collect();
        assert_eq!(chars, "[ ]");

        // カーソルのある行は記法をそのまま見せる
        let revealed = run(SAMPLE, true, Some((3, 13)));
        assert_eq!(hidden(&revealed), "");
        assert!(revealed.decos.iter().all(|d| d.kind == DecoKind::Code));
        // ソース表示では何も畳まない
        let source = run(SAMPLE, false, None);
        assert_eq!(hidden(&source), "");
        assert!(source.decos.iter().all(|d| d.kind == DecoKind::Code));
    }

    #[test]
    fn image_only_lines() {
        assert_eq!(image_only(" ![[a b.png|300]] ").as_deref(), Some("a b.png"));
        assert_eq!(image_only("![alt](img/a%20b.png)").as_deref(), Some("img/a b.png"));
        assert_eq!(image_only("![alt](https://x/y.png)"), None);
        assert_eq!(image_only("text ![[a.png]]"), None);
        assert!(is_rule("---") && is_rule("* * *") && !is_rule("--") && !is_rule("-a-"));
    }
}
