//! エディタ用のMarkdownシンタックスハイライトと、カーソル位置まわりの補助関数。

use crate::fonts;
use crate::highlight;
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Align, Color32, FontFamily, FontId, Stroke, Vec2};
use robsidian_core::{parse, LinkKind};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq)]
pub struct Colors {
    pub dark: bool,
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
    pub diff_add: Color32,
    pub find_bg: Color32,
    pub find_current: Color32,
    pub diff_del: Color32,
    pub warn_bg: Color32,
}

impl Colors {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark,
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
                diff_add: Color32::from_rgb(0x1f, 0x3d, 0x2a),
                find_bg: Color32::from_rgb(0x4d, 0x43, 0x1a),
                find_current: Color32::from_rgb(0x9a, 0x6a, 0x10),
                diff_del: Color32::from_rgb(0x4a, 0x22, 0x26),
                warn_bg: Color32::from_rgb(0x4a, 0x3a, 0x12),
            }
        } else {
            Self {
                dark,
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
                diff_add: Color32::from_rgb(0xd8, 0xf5, 0xdf),
                find_bg: Color32::from_rgb(0xff, 0xf0, 0xa8),
                find_current: Color32::from_rgb(0xff, 0xb8, 0x4d),
                diff_del: Color32::from_rgb(0xfb, 0xdd, 0xdd),
                warn_bg: Color32::from_rgb(0xfd, 0xf0, 0xc8),
            }
        }
    }
}

/// 記法文字を畳むときのフォントサイズ。0にはできない（幅0・高さ0の文字として残す）。
const HIDDEN_SIZE: f32 = 0.01;

/// 本文の行の高さ（文字サイズに対する倍率）。
const LINE_HEIGHT: f32 = 1.5;

/// 本文1行ぶんの高さ。
pub fn line_height(size: f32) -> f32 {
    size * LINE_HEIGHT
}

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
    /// 埋め込んだノート（範囲は行全体、中身は `ノート#見出し`）
    NoteEmbed(String),
    /// コールアウトの1行（範囲は行全体）。タイトルが無い最初の行には、種類名を描く位置（文字位置）が付く
    Callout {
        color: Color32,
        first: bool,
        label: Option<(String, usize)>,
    },
    /// 表（範囲は表全体。先頭の行が見出し）
    Table(Vec<TableRow>),
    /// 数式。表示数式（`$$…$$`）は範囲がブロック全体、行内数式は幅を確保した1文字
    Math { tex: String, display: bool },
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
    /// カーソル（選択範囲）の文字位置（両端含む）。これに触れている要素だけ記法をそのまま見せる
    pub cursor: Option<(usize, usize)>,
    /// 画像リンク先 → 表示サイズ。読み込めない・未読み込みなら None
    pub image: &'a dyn Fn(&str) -> Option<Vec2>,
    /// 埋め込んだノート（`ノート#見出し`）を描く高さ。ノートでなければ None
    pub note_embed: &'a dyn Fn(&str) -> Option<f32>,
    /// 数式（TeX, 表示数式か）を描いたときの大きさ。描けなければ None
    pub math: &'a dyn Fn(&str, bool) -> Option<Vec2>,
    /// frontmatterを畳む（プロパティ欄を出しているとき）
    pub hide_frontmatter: bool,
    /// 検索で一致した範囲（バイト位置）と、そのうち選択中のものの番号
    pub marks: &'a [Range<usize>],
    pub current_mark: Option<usize>,
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
    live: bool,
    cursor: Option<(usize, usize)>,
    math: &'a dyn Fn(&str, bool) -> Option<Vec2>,
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

    /// この文字範囲の記法を畳んでよいか（Live Previewで、カーソルが触れていない）。
    fn hidden(&self, chars: Range<usize>) -> bool {
        self.live
            && !self
                .cursor
                .is_some_and(|(a, b)| a <= chars.end && chars.start <= b)
    }

    /// いま積んでいる位置から `text` ぶんの文字範囲。
    fn ahead(&self, text: &str) -> Range<usize> {
        self.chars..self.chars + text.chars().count()
    }

    /// 行頭の字下げ。タブ文字は狭く描かれるので、空白4つぶん程度まで広げる。
    fn indent_fmt(&self, indent: &str) -> TextFormat {
        let mut f = self.fmt(self.c.text);
        if indent.contains('\t') {
            f.extra_letter_spacing = self.size * 1.2;
        }
        f
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
        self.fmt(if hide {
            Color32::TRANSPARENT
        } else {
            self.c.dim
        })
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
    fn inline(&mut self, line: &str, base: Color32) {
        let b = line.as_bytes();
        let mut plain_from = 0;
        let mut i = 0;
        let maths = if line.contains('$') {
            robsidian_core::math::inline_spans(line)
        } else {
            Vec::new()
        };
        // (記法の開始, 中身の開始, 中身の終了, 記法の終了, 中身の書式)
        type Token = (usize, usize, usize, usize, TextFormat);
        while i < b.len() {
            if let Some(span) = maths.iter().find(|r| r.start == i) {
                self.push(&line[plain_from..i], self.fmt(base));
                self.inline_math(&line[span.clone()]);
                plain_from = span.end;
                i = span.end;
                continue;
            }
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
                    let opens = b
                        .get(i + n)
                        .is_some_and(|c| !c.is_ascii_whitespace() && *c != ch);
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
                                    (b'*', true) => {
                                        f.color = self.c.strong;
                                        f.font_id.family = fonts::bold();
                                    }
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
                    // カーソルがこの要素に触れている間だけ記法を見せる
                    let hide = self.hidden(self.ahead(&line[start..end]));
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

    /// 行内数式 `$…$`。ライブ表示でカーソルが触れていなければ、記法を畳んで描いた数式の幅を確保する
    /// （上に数式を描く）。行の高さは、記法を見せるときも同じにする（カーソルの出入りで動かないように）。
    fn inline_math(&mut self, src: &str) {
        let tex = &src[1..src.len() - 1];
        let drawn = if self.live {
            (self.math)(tex, false)
        } else {
            None
        };
        let Some(sz) = drawn else {
            self.push(src, self.fmt(self.c.code));
            return;
        };
        let row = Some((self.size * LINE_HEIGHT).max(sz.y + 4.0));
        let hide = self.hidden(self.ahead(src));
        if hide {
            // 先頭の `$` を極小・透明にして、数式の幅だけ字間を空ける
            let mut slot = self.mark(true);
            slot.extra_letter_spacing = sz.x;
            slot.line_height = row;
            let at = self.push(&src[..1], slot);
            let mut rest = self.mark(true);
            rest.line_height = row;
            self.push(&src[1..], rest);
            self.deco(
                DecoKind::Math {
                    tex: tex.to_string(),
                    display: false,
                },
                at,
            );
        } else {
            let mut f = self.fmt(self.c.code);
            f.line_height = row;
            self.push(src, f);
        }
    }

    /// 行頭の記号（リスト・タスク・引用）を積み、残りの本文の開始位置と文字色を返す。
    /// 行頭の記号（リスト・タスク・引用）を積み、残りの本文の開始位置と文字色、
    /// 引用の縦線を描くかどうかを返す。記号は、カーソルが触れている間だけそのまま見せる。
    fn block_prefix(&mut self, line: &str) -> (usize, Color32, bool) {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let text = self.c.text;
        if trimmed.starts_with("> ") || trimmed == ">" {
            let end = (indent + 2).min(line.len());
            let hide = self.hidden(self.ahead(&line[..end]));
            self.push(&line[..indent], self.indent_fmt(&line[..indent]));
            self.push(&line[indent..end], self.ghost(hide));
            return (end, self.c.quote, hide);
        }
        let bullet = ["- ", "* ", "+ "].iter().any(|m| trimmed.starts_with(m));
        if bullet {
            let rest = &trimmed[2..];
            let checked = rest.starts_with("[x] ") || rest.starts_with("[X] ");
            if checked || rest.starts_with("[ ] ") {
                // `- [ ] ` → 先頭の `- ` を畳み、`[ ]` の位置にチェックボックスを描く
                let hide = self.hidden(self.ahead(&line[..indent + 5]));
                self.push(&line[..indent], self.indent_fmt(&line[..indent]));
                self.push(&line[indent..indent + 2], self.mark(hide));
                let range = self.push(&line[indent + 2..indent + 5], self.ghost(hide));
                if hide {
                    self.deco(DecoKind::Checkbox(checked), range);
                }
                self.push(" ", self.fmt(text));
                return (indent + 6, if checked { self.c.dim } else { text }, false);
            }
            let hide = self.hidden(self.ahead(&line[..indent + 1]));
            self.push(&line[..indent], self.indent_fmt(&line[..indent]));
            let range = self.push(&line[indent..indent + 1], self.ghost(hide));
            if hide {
                self.deco(DecoKind::Bullet, range);
            }
            self.push(" ", self.fmt(text));
            return (indent + 2, text, false);
        }
        let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && trimmed[digits..].starts_with(". ") {
            let end = indent + digits + 2;
            self.push(&line[..end], self.fmt(self.c.dim));
            return (end, text, false);
        }
        (0, text, false)
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

/// `n` 行目から始まる表示数式 `$$…$$`。（最後の行, 中身のTeX）
fn display_math(lines: &[&str], n: usize) -> Option<(usize, String)> {
    let first = lines[n].trim_end_matches(['\n', '\r']).trim();
    let after = first.strip_prefix("$$")?;
    if let Some(tex) = after.strip_suffix("$$") {
        // 1行の `$$…$$`（`$$` だけの行は開きとみなす）
        if !after.is_empty() {
            return (!tex.trim().is_empty()).then(|| (n, tex.to_string()));
        }
    }
    let mut tex = after.to_string();
    for (k, raw) in lines.iter().enumerate().skip(n + 1).take(200) {
        let l = raw.trim_end_matches(['\n', '\r']).trim_end();
        if let Some(last) = l.strip_suffix("$$") {
            tex.push('\n');
            tex.push_str(last);
            return (!tex.trim().is_empty()).then_some((k, tex));
        }
        tex.push('\n');
        tex.push_str(l);
    }
    None
}

/// `| a | b |` をセルに分ける（`\|` は区切りとして扱わない）。
fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t
        .strip_suffix('|')
        .filter(|r| !r.ends_with('\\'))
        .unwrap_or(t);
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
    let mut p = Painter {
        job: LayoutJob::default(),
        decos: Vec::new(),
        chars: 0,
        size,
        c: colors,
        live: true,
        cursor: None,
        // 表のセルの数式は、記法を見せたまま色だけ付ける（上に描く仕組みが無いため）
        math: &|_: &str, _: bool| None,
    };
    p.inline(text, color);
    p.job
}

/// 行が `![[ノート]]`（別名は除く）だけで出来ていれば、`ノート#見出し` を返す。
pub fn embed_only(line: &str) -> Option<String> {
    let inner = line.trim().strip_prefix("![[")?.strip_suffix("]]")?;
    if inner.contains("]]") || inner.contains("[[") {
        return None;
    }
    let target = inner.split('|').next()?.trim();
    (!target.is_empty()).then(|| target.to_string())
}

/// コールアウトの最初の行（`> [!種類]` 、折りたたみの `+` `-` 付きも）なら、
/// (小文字の種類, 引用記号の後ろから数えた記法の長さ)。
pub fn callout_kind(line: &str) -> Option<(String, usize)> {
    let t = line.trim_start().strip_prefix('>')?.trim_start();
    let rest = t.strip_prefix("[!")?;
    let end = rest.find(']')?;
    let kind = &rest[..end];
    if kind.is_empty()
        || !kind
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let mut len = 2 + end + 1;
    if matches!(t.as_bytes().get(len), Some(b'+' | b'-')) {
        len += 1;
    }
    Some((kind.to_lowercase(), len))
}

/// コールアウトの種類ごとの色（Obsidianの既定に近い色）。
pub fn callout_color(kind: &str) -> Color32 {
    match kind {
        "abstract" | "summary" | "tldr" => Color32::from_rgb(0x00, 0xb0, 0xd0),
        "tip" | "hint" | "important" => Color32::from_rgb(0x00, 0xbf, 0xa5),
        "success" | "check" | "done" => Color32::from_rgb(0x08, 0xb9, 0x4e),
        "question" | "help" | "faq" => Color32::from_rgb(0xe0, 0xac, 0x00),
        "warning" | "caution" | "attention" => Color32::from_rgb(0xec, 0x75, 0x00),
        "failure" | "fail" | "missing" | "danger" | "error" | "bug" => {
            Color32::from_rgb(0xe9, 0x31, 0x47)
        }
        "example" => Color32::from_rgb(0x7c, 0x4d, 0xff),
        "quote" | "cite" => Color32::from_rgb(0x9e, 0x9e, 0x9e),
        _ => Color32::from_rgb(0x44, 0x8a, 0xff),
    }
}

/// 表示用の種類名（先頭を大文字にする）。
pub fn callout_label(kind: &str) -> String {
    let mut c = kind.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn is_rule(trimmed: &str) -> bool {
    let mut chars = trimmed.chars().filter(|c| !c.is_whitespace());
    let Some(first) = chars.next() else {
        return false;
    };
    matches!(first, '-' | '*' | '_') && chars.clone().all(|c| c == first) && chars.count() >= 2
}

pub fn layout(text: &str, opt: &Options) -> Layout {
    let mut p = Painter {
        job: LayoutJob::default(),
        decos: Vec::new(),
        chars: 0,
        size: opt.size,
        c: opt.colors,
        live: opt.live,
        cursor: opt.cursor,
        math: opt.math,
    };
    let size = opt.size;
    let mut in_frontmatter = text.starts_with("---\n") || text.starts_with("---\r\n");
    let mut fence: Option<(u8, usize)> = None;
    // 処理中の表: (最後の行, 整形表示するか, 表の開始文字位置, 集めた行) と、その最初の行
    let mut table: Option<(usize, bool, usize, Vec<TableRow>)> = None;
    let mut first_line = 0;
    // 処理中のコールアウト: (種類, 最初の行)
    let mut callout: Option<(String, usize)> = None;
    // 処理中の表示数式: (最後の行, 整形表示するか, 開始文字位置, 行の高さ, TeX)
    let mut math_block: Option<(usize, bool, usize, f32, String)> = None;
    // 処理中のコードブロックの色分け結果: (本文の最初の行, 行ごとの (色, バイト数))
    let mut code: Option<(usize, highlight::Lines)> = None;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    fn content(raw: &str) -> &str {
        raw.trim_end_matches(['\n', '\r'])
    }

    for (n, raw) in lines.iter().copied().enumerate() {
        let line = content(raw);
        let eol = &raw[line.len()..];
        let trimmed = line.trim_start();
        // 行全体が1つの要素（水平線・画像・コードフェンス）のときの判定
        let hide = p.hidden(p.ahead(line));

        if in_frontmatter {
            // ライブ表示では、frontmatterはプロパティ欄で見せるので本文からは畳む
            let format = if opt.hide_frontmatter {
                p.mark(true)
            } else {
                p.mono(p.c.dim)
            };
            p.push(raw, format);
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
        let closes =
            matches!((marker, fence), (Some((c, l)), Some((oc, ol))) if c == oc && l >= ol);
        if fence.is_some() || marker.is_some() {
            let opens = fence.is_none();
            let start = p.chars;
            if opens || closes {
                // ``` は畳むが、行の高さは固定する（カーソルの出入りで下の行が動かないように）。
                // 開く側は言語名だけ薄く残す。
                let ticks = line.len() - trimmed.len() + marker.map_or(0, |(_, len)| len);
                let row = Some(size * 1.3);
                let mut mark = if hide { p.mark(true) } else { p.mono(p.c.dim) };
                mark.line_height = row;
                let mut label = p.mono(p.c.dim);
                label.line_height = row;
                p.push(&line[..ticks], mark);
                p.push(&line[ticks..], label.clone());
                p.push(eol, label);
            } else {
                let colored = code
                    .as_ref()
                    .and_then(|(first, spans)| spans.get(n - first));
                match colored {
                    Some(spans) => {
                        // 色分け済みの断片は、行末の改行まで含めて元の行をちょうど覆う
                        let mut at = 0;
                        for &(color, len) in spans {
                            let end = (at + len).min(raw.len());
                            p.push(&raw[at..end], p.mono(color));
                            at = end;
                        }
                        p.push(&raw[at..], p.mono(p.c.code));
                    }
                    None => {
                        p.push(raw, p.mono(p.c.code));
                    }
                }
            }
            p.deco(DecoKind::Code, start..start + line.chars().count());
            if opens {
                fence = marker;
                let lang = trimmed.trim_start_matches(['`', '~']).trim();
                let lang = lang.split([' ', ',', '{']).next().unwrap_or("");
                // ブロックの終わり（閉じフェンスの手前、無ければ文書の最後）までを一度に色分けする
                let (ch, len) = marker.unwrap_or((b'`', 3));
                let body_end = (n + 1..lines.len())
                    .find(|&i| {
                        let t = content(lines[i]).trim_start();
                        let run = t.bytes().take_while(|&c| c == ch).count();
                        run >= len && t[run..].trim().is_empty()
                    })
                    .unwrap_or(lines.len());
                code = highlight::code(lang, &lines[n + 1..body_end], opt.colors.dark)
                    .map(|spans| (n + 1, spans));
            } else if closes {
                fence = None;
                code = None;
            }
            continue;
        }

        let level = trimmed.bytes().take_while(|&c| c == b'#').count();
        let is_heading = (1..=6).contains(&level)
            && (trimmed.len() == level || trimmed[level..].starts_with(' '));
        if is_heading {
            let scale = [1.6, 1.4, 1.22, 1.1, 1.05, 1.0][level - 1];
            let mut format = p.fmt(p.c.heading);
            format.font_id.family = fonts::bold();
            format.font_id.size = size * scale;
            format.line_height = Some(size * scale * LINE_HEIGHT);
            let marker_end = (line.len() - trimmed.len() + level + 1).min(line.len());
            // `# ` は、カーソルがそこに触れているときだけ見せる
            let hide = p.hidden(p.ahead(&line[..marker_end]));
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
            .filter(|_| opt.live)
            .and_then(|t| (opt.image)(&t).map(|s| (t, s)))
        {
            // 記法の行（1行ぶん）の下に画像の場所を確保する。行の高さは、記法を見せるときも
            // 畳むときも同じにして、カーソルの出入りで下の内容が動かないようにする。
            let mut format = if hide { p.mark(true) } else { p.fmt(p.c.dim) };
            format.line_height = Some(line_height(size) + image_size.y + 8.0);
            format.valign = Align::TOP;
            let range = p.push(line, format);
            p.deco(DecoKind::Image(target), range);
        } else if let Some((target, height)) = embed_only(line)
            .filter(|_| opt.live)
            .and_then(|t| (opt.note_embed)(&t).map(|h| (t, h)))
        {
            // 画像と同じく、記法の行の下に埋め込みの場所を確保する（高さは前回の描画で測った値）
            let mut format = if hide { p.mark(true) } else { p.fmt(p.c.dim) };
            format.line_height = Some(line_height(size) + height + 8.0);
            format.valign = Align::TOP;
            let range = p.push(line, format);
            p.deco(DecoKind::NoteEmbed(target), range);
        } else if math_block.is_some()
            || (opt.live && trimmed.starts_with("$$")) && {
                // 表示数式の開始: 閉じの `$$` があり、描けるときだけ扱う
                let found = display_math(&lines, n)
                    .and_then(|(end, tex)| (opt.math)(&tex, true).map(|sz| (end, tex, sz)));
                if let Some((end, tex, sz)) = &found {
                    let count = (end - n + 1) as f32;
                    let block: usize = lines[n..=*end].iter().map(|l| l.chars().count()).sum();
                    let pretty = p.hidden(p.chars..p.chars + block);
                    // ブロックの高さを数式に合わせる（ソース表示でも同じ高さにする）
                    let row = line_height(size).max((sz.y + size * 1.2) / count);
                    math_block = Some((*end, pretty, p.chars, row, tex.clone()));
                }
                found.is_some()
            }
        {
            let (end, pretty, start, row, tex) = math_block.take().expect("set above");
            let mut format = if pretty {
                p.mark(true)
            } else {
                p.fmt(p.c.code)
            };
            format.line_height = Some(row);
            p.push(line, format.clone());
            if n < end {
                math_block = Some((end, pretty, start, row, tex));
            } else if pretty {
                p.deco(DecoKind::Math { tex, display: true }, start..p.chars);
            }
            // 改行も同じ高さにする（ブロックの各行の高さをそろえる）
            p.push(eol, format);
            continue;
        } else if table.is_some() || trimmed.starts_with('|') {
            if table.is_none() {
                // 表の開始: 2行目が区切り行なら、連続する `|` 行を1つの表として扱う
                let is_table = lines
                    .get(n + 1)
                    .is_some_and(|l| is_table_separator(content(l)));
                let end = if is_table {
                    (n + 1..lines.len())
                        .take_while(|&i| content(lines[i]).trim_start().starts_with('|'))
                        .last()
                        .unwrap_or(n)
                } else {
                    n
                };
                // 表の中にカーソルがある間は、表全体をソースのまま見せる
                let block: usize = lines[n..=end].iter().map(|l| l.chars().count()).sum();
                let pretty = is_table && p.hidden(p.chars..p.chars + block);
                table = Some((end, pretty, p.chars, Vec::new()));
                first_line = n;
            }
            let (end, pretty, start, mut rows) = table.take().expect("set above");
            // 表の行の高さは、整形表示でもソース表示でも同じにする（カーソルが表に入っても
            // 下の内容が動かないように）。区切り行は低くして、整形表示では見出し行の一部として描く。
            let separator = n == first_line + 1 && is_table_separator(line);
            let row_height = opt.live.then_some(size * if separator { 1.0 } else { 1.7 });
            let mut format = if !pretty {
                p.mono(p.c.text)
            } else if separator {
                p.mark(true)
            } else {
                p.ghost(true)
            };
            format.line_height = row_height;
            let chars = p.push(line, format);
            if pretty && !separator {
                rows.push(TableRow {
                    chars,
                    cells: table_cells(line),
                });
            }
            if n < end {
                table = Some((end, pretty, start, rows));
            } else if pretty {
                p.deco(DecoKind::Table(rows), start..p.chars);
            }
        } else {
            let start = p.chars;
            if !trimmed.starts_with('>') {
                callout = None;
            } else if callout.is_none() && opt.live {
                callout = callout_kind(line).map(|(kind, _)| (kind, n));
            }
            let (body, color, quote_bar) = p.block_prefix(line);
            match callout.clone() {
                Some((kind, first)) => {
                    let accent = callout_color(&kind);
                    let mut label = None;
                    if first == n {
                        // `[!種類]` はカーソルが触れていなければ畳み、タイトルを太字で見せる
                        let rest = &line[body..];
                        let len = callout_kind(line).map_or(0, |(_, l)| l).min(rest.len());
                        let marker = p.ahead(&rest[..len]);
                        let hide_marker = p.hidden(marker.clone());
                        p.push(&rest[..len], p.mark(hide_marker));
                        let mut title = p.fmt(accent);
                        title.font_id.family = fonts::bold();
                        p.push(&rest[len..], title);
                        if hide_marker && rest[len..].trim().is_empty() {
                            label = Some((callout_label(&kind), marker.start));
                        }
                    } else {
                        p.inline(&line[body..], p.c.text);
                    }
                    p.deco(
                        DecoKind::Callout {
                            color: accent,
                            first: first == n,
                            label,
                        },
                        start..p.chars,
                    );
                }
                None => {
                    p.inline(&line[body..], color);
                    if quote_bar {
                        p.deco(DecoKind::Quote, start..p.chars);
                    }
                }
            }
        }
        p.push(eol, p.fmt(p.c.text));
    }
    let mut job = p.job;
    paint_marks(&mut job, opt.marks, opt.current_mark, opt.colors);
    Layout {
        job,
        decos: p.decos,
    }
}

/// 検索の一致範囲に背景色を付ける（区間を一致の境界で分割する）。
fn paint_marks(job: &mut LayoutJob, marks: &[Range<usize>], current: Option<usize>, c: &Colors) {
    if marks.is_empty() {
        return;
    }
    let sections = std::mem::take(&mut job.sections);
    for section in sections {
        let (s, e) = (section.byte_range.start.0, section.byte_range.end.0);
        let mut cuts = vec![s, e];
        // 一致は位置順に並んでいるので、区間に重なるものだけを見る
        let first = marks.partition_point(|m| m.end <= s);
        for m in marks[first..].iter().take_while(|m| m.start < e) {
            cuts.push(m.start.clamp(s, e));
            cuts.push(m.end.clamp(s, e));
        }
        cuts.sort_unstable();
        cuts.dedup();
        for (i, w) in cuts.windows(2).enumerate() {
            let mut piece = section.clone();
            piece.byte_range = w[0].into()..w[1].into();
            if i > 0 {
                piece.leading_space = 0.0;
            }
            let hit = marks[first..]
                .iter()
                .take_while(|m| m.start < e)
                .position(|m| m.start <= w[0] && w[1] <= m.end);
            if let Some(k) = hit {
                piece.format.background = if current == Some(first + k) {
                    c.find_current
                } else {
                    c.find_bg
                };
            }
            job.sections.push(piece);
        }
    }
}

pub fn byte_of_char(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(b, _)| b)
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

/// カーソル位置（文字境界のバイト位置）にあるリンクのリンク先。見出しの指定があれば `ノート#見出し`。
pub fn link_at(text: &str, byte_pos: usize) -> Option<String> {
    let start = text[..byte_pos].rfind('\n').map_or(0, |i| i + 1);
    let end = text[byte_pos..]
        .find('\n')
        .map_or(text.len(), |i| byte_pos + i);
    let line = &text[start..end];
    let pos = byte_pos - start;
    let found = parse::parse(line).links.into_iter().find_map(|l| {
        let range = match l.kind {
            LinkKind::Wiki => l.full.clone(),
            LinkKind::Markdown => {
                let open = line[..l.span.start].rfind('[').unwrap_or(l.span.start);
                open..(l.span.end + 1).min(line.len())
            }
        };
        let target = match &l.heading {
            Some(h) => format!("{}#{h}", l.target),
            None => l.target,
        };
        (range.start <= pos && pos <= range.end).then_some(target)
    });
    // `[[#見出し]]`（同じノート内のリンク）は索引の対象外なので、ここで拾う
    found.or_else(|| {
        let open = line[..pos.min(line.len())].rfind("[[#")?;
        let close = open + line[open..].find("]]")?;
        let inner = &line[open + 2..close];
        (pos <= close + 2 && !inner.contains('['))
            .then(|| inner.split('|').next().unwrap_or(inner).trim().to_string())
    })
}

/// カーソルが `[[入力途中` の中にあれば、その開始バイト位置（`[[` の直後）を返す。
pub fn completion_start(text: &str, cursor: usize) -> Option<usize> {
    let line_start = text[..cursor].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line_start..cursor];
    let open = before.rfind("[[")?;
    let typed = &before[open + 2..];
    // `#` は見出しの指定として1つだけ許す（`[[ノート#見出し`）
    if typed.contains(['|', '[', ']']) || typed.matches('#').count() > 1 || typed.len() > 80 {
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
        let h = "x [[Note#節|別名]] y [[#自分の節]]";
        assert_eq!(link_at(h, 5).as_deref(), Some("Note#節"));
        assert_eq!(link_at(h, h.len() - 2).as_deref(), Some("#自分の節"));
        assert_eq!(link_at(h, 0), None);
        let k = "[[Note#";
        assert_eq!(&k[completion_start(k, k.len()).unwrap()..], "Note#");
        assert_eq!(completion_start("[[a#b#", 6), None);
        assert_eq!(link_at(t, 0), None);
        let md = t.find("b.md").unwrap();
        assert_eq!(link_at(t, md + 1).as_deref(), Some("b.md"));
        let start = completion_start(t, t.len()).unwrap();
        assert_eq!(&t[start..], "Par");
        assert_eq!(completion_start(t, 3), None);
        assert_eq!(line_start(t, 1), t.find('\n').unwrap() + 1);
        assert_eq!(char_of_byte(t, byte_of_char(t, 5)), 5);
    }

    fn run(text: &str, live: bool, cursor: Option<(usize, usize)>) -> Layout {
        let colors = Colors::new(true);
        let image = |t: &str| (t == "pic.png").then_some(Vec2::new(100.0, 50.0));
        layout(
            text,
            &Options {
                marks: &[],
                current_mark: None,
                size: 14.0,
                colors: &colors,
                live,
                cursor,
                image: &image,
                note_embed: &|_: &str| None,
                math: &|tex: &str, display: bool| {
                    Some(Vec2::new(
                        tex.len() as f32 * 7.0,
                        if display { 40.0 } else { 18.0 },
                    ))
                },
                hide_frontmatter: false,
            },
        )
    }

    const SAMPLE: &str = "---\na: b\n---\n# H\n- [ ] `c` [[l|別名]] **b** *i* ~~s~~ ==m== #tag [x](y)\n- item\n> quote\n---\n![[pic.png]]\n```\ncode\n```\n| a | b |\n|---|---|\n| 1 | x \\| y |\nsnake_case_name tail";

    #[test]
    fn layout_never_changes_the_text() {
        for (live, cursor) in [(false, None), (true, None), (true, Some((20, 60)))] {
            let out = run(SAMPLE, live, cursor);
            assert_eq!(out.job.text, SAMPLE);
            let total: usize = out
                .job
                .sections
                .iter()
                .map(|s| s.byte_range.end.0 - s.byte_range.start.0)
                .sum();
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
        let slice =
            |r: &Range<usize>| -> String { SAMPLE.chars().skip(r.start).take(r.len()).collect() };
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
        let DecoKind::Table(rows) = &kinds[8] else {
            panic!("expected a table")
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (slice(&rows[0].chars).as_str(), &rows[0].cells),
            ("| a | b |", &vec!["a".to_string(), "b".into()])
        );
        assert_eq!(rows[1].cells, ["1", "x | y"]);
        assert_eq!(slice(&rows[1].chars), "| 1 | x \\| y |");
        // チェックボックスの範囲は `[ ]` の3文字
        let cb = &live.decos[0].chars;
        let chars: String = SAMPLE.chars().skip(cb.start).take(cb.len()).collect();
        assert_eq!(chars, "[ ]");

        // カーソルが触れている要素だけ記法を見せる（同じ行の他の要素は畳んだまま）
        let at = |needle: &str| SAMPLE[..SAMPLE.find(needle).unwrap()].chars().count();
        let bold = at("**b**") + 3;
        let on_bold = run(SAMPLE, true, Some((bold, bold)));
        assert_eq!(
            hidden(&on_bold),
            "# ¦- ¦`¦`¦[[l|¦]]¦*¦*¦~~¦~~¦==¦==¦[¦](y)¦![[pic.png]]¦```¦```¦|---|---|"
        );
        // 行頭に置くと、その行の行頭記号だけが見える
        let task = at("- [ ]");
        let on_task = run(SAMPLE, true, Some((task, task)));
        assert!(hidden(&on_task).starts_with("# ¦`¦`¦[[l|"));
        assert!(!on_task
            .decos
            .iter()
            .any(|d| matches!(d.kind, DecoKind::Checkbox(_))));
        // 表の中に置くと、表全体がソース表示に戻る
        let cell = at("| 1 |") + 2;
        let on_table = run(SAMPLE, true, Some((cell, cell)));
        assert!(!hidden(&on_table).contains("|---|"));
        assert!(!on_table
            .decos
            .iter()
            .any(|d| matches!(d.kind, DecoKind::Table(_))));
        // 全選択すればすべての記法が見える。ソース表示では何も畳まない
        let all = run(SAMPLE, true, Some((0, SAMPLE.chars().count())));
        assert_eq!(hidden(&all), "");
        let source = run(SAMPLE, false, None);
        assert_eq!(hidden(&source), "");
        assert!(source.decos.iter().all(|d| d.kind == DecoKind::Code));
    }

    #[test]
    fn search_marks_are_painted() {
        let colors = Colors::new(true);
        let text = "# Rust\n**rust** and rust";
        let marks = [2..6, 9..13, 20..24];
        let image = |_: &str| None;
        let out = layout(
            text,
            &Options {
                marks: &marks,
                current_mark: Some(1),
                size: 14.0,
                colors: &colors,
                live: false,
                cursor: None,
                image: &image,
                note_embed: &|_: &str| None,
                math: &|tex: &str, display: bool| {
                    Some(Vec2::new(
                        tex.len() as f32 * 7.0,
                        if display { 40.0 } else { 18.0 },
                    ))
                },
                hide_frontmatter: false,
            },
        );
        assert_eq!(out.job.text, text);
        let painted: Vec<(&str, bool)> = out
            .job
            .sections
            .iter()
            .filter(|s| {
                s.format.background == colors.find_bg || s.format.background == colors.find_current
            })
            .map(|s| {
                let r = s.byte_range.start.0..s.byte_range.end.0;
                (&text[r], s.format.background == colors.find_current)
            })
            .collect();
        assert_eq!(painted, [("Rust", false), ("rust", true), ("rust", false)]);
        // 区間は隙間なく本文全体を覆う
        let mut at = 0;
        for s in &out.job.sections {
            assert_eq!(s.byte_range.start.0, at);
            at = s.byte_range.end.0;
        }
        assert_eq!(at, text.len());
    }

    #[test]
    fn frontmatter_can_be_folded() {
        let colors = Colors::new(true);
        let text = "---\na: 1\n---\nbody";
        let image = |_: &str| None;
        let out = layout(
            text,
            &Options {
                marks: &[],
                current_mark: None,
                size: 14.0,
                colors: &colors,
                live: true,
                cursor: None,
                image: &image,
                note_embed: &|_: &str| None,
                math: &|tex: &str, display: bool| {
                    Some(Vec2::new(
                        tex.len() as f32 * 7.0,
                        if display { 40.0 } else { 18.0 },
                    ))
                },
                hide_frontmatter: true,
            },
        );
        let hidden: String = out
            .job
            .sections
            .iter()
            .filter(|s| s.format.font_id.size == HIDDEN_SIZE)
            .map(|s| &text[s.byte_range.start.0..s.byte_range.end.0])
            .collect();
        assert_eq!(hidden, "---\na: 1\n---\n");
    }

    #[test]
    fn callouts_and_note_embeds() {
        let colors = Colors::new(true);
        let text = "> [!tip]- Title\n> body\nafter\n> [!note]\n> b\n![[Other#h]]\nend";
        let image = |_: &str| None;
        let embed = |t: &str| (t == "Other#h").then_some(80.0);
        let out = layout(
            text,
            &Options {
                marks: &[],
                current_mark: None,
                size: 14.0,
                colors: &colors,
                live: true,
                cursor: None,
                image: &image,
                note_embed: &embed,
                math: &|_: &str, _: bool| None,
                hide_frontmatter: false,
            },
        );
        let kinds: Vec<String> = out
            .decos
            .iter()
            .map(|d| match &d.kind {
                DecoKind::Callout { first, label, .. } => {
                    format!(
                        "callout:{first}:{}",
                        label.as_ref().map_or("-", |l| l.0.as_str())
                    )
                }
                DecoKind::NoteEmbed(t) => format!("embed:{t}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "callout:true:-",
                "callout:false:-",
                "callout:true:Note",
                "callout:false:-",
                "embed:Other#h"
            ]
        );
        assert_eq!(callout_kind("> [!TIP]+ x"), Some(("tip".into(), 7)));
        assert_eq!(callout_kind("> [!] x"), None);
        assert_eq!(callout_kind("> [link](x)"), None);
        assert_eq!(embed_only(" ![[a b|alias]] ").as_deref(), Some("a b"));
        assert_eq!(embed_only("x ![[a]]"), None);
        assert_eq!(callout_label("warning"), "Warning");
    }

    #[test]
    fn row_heights_do_not_depend_on_the_cursor() {
        // カーソルの出入りで行の高さが変わると表示が上下に動くので、どの要素でも一定に保つ
        let text = "# H\n**b** [[l|a]]\n- [ ] t\n> q\n```rust\nfn f() {}\n```\n![[pic.png]]\n| a | b |\n|---|---|\n| 1 | 2 |\n> [!note]\n> c\n式 $x^2$ と $5 $10\n$$\n\\frac{a}{b}\n$$\n$$y$$\nend";
        let heights = |cursor| -> Vec<Option<u32>> {
            let out = run(text, true, cursor);
            let mut rows = vec![None; text.lines().count()];
            for section in &out.job.sections {
                let line = text[..section.byte_range.start.0].matches('\n').count();
                let size = section.format.font_id.size;
                let h = section
                    .format
                    .line_height
                    .unwrap_or(if size > 1.0 { size } else { 0.0 });
                rows[line] = rows[line].max(Some((h * 100.0) as u32));
            }
            rows
        };
        let unfocused = heights(None);
        for cursor in 0..=text.chars().count() {
            assert_eq!(
                heights(Some((cursor, cursor))),
                unfocused,
                "cursor at {cursor}"
            );
        }
    }

    #[test]
    fn math_is_drawn_over_reserved_space() {
        let text = "a $x^2$ b\n$$\n\\frac{1}{2}\n$$\n価格 $5 と $10\n$$ 閉じない";
        let out = run(text, true, None);
        assert_eq!(out.job.text, text);
        let maths: Vec<(String, bool)> = out
            .decos
            .iter()
            .filter_map(|d| match &d.kind {
                DecoKind::Math { tex, display } => Some((tex.clone(), *display)),
                _ => None,
            })
            .collect();
        assert_eq!(
            maths,
            [
                ("x^2".to_string(), false),
                ("\n\\frac{1}{2}\n".to_string(), true)
            ]
        );
        // 行内数式の場所には、描いた幅（テストでは 文字数×7）だけ字間を空ける
        let slot = out
            .job
            .sections
            .iter()
            .find(|s| s.format.extra_letter_spacing > 0.0)
            .unwrap();
        assert_eq!(slot.format.extra_letter_spacing, 21.0);
        // カーソルが数式に入ると、記法をそのまま見せる
        let on = run(text, true, Some((3, 3)));
        assert!(!on
            .decos
            .iter()
            .any(|d| matches!(&d.kind, DecoKind::Math { display: false, .. })));
        // ソース表示では何もしない
        assert!(run(text, false, None).decos.is_empty());
        assert_eq!(display_math(&["$$x$$"], 0), Some((0, "x".into())));
        assert_eq!(display_math(&["$$\n", "y\n"], 0), None);
    }

    #[test]
    fn image_only_lines() {
        assert_eq!(image_only(" ![[a b.png|300]] ").as_deref(), Some("a b.png"));
        assert_eq!(
            image_only("![alt](img/a%20b.png)").as_deref(),
            Some("img/a b.png")
        );
        assert_eq!(image_only("![alt](https://x/y.png)"), None);
        assert_eq!(image_only("text ![[a.png]]"), None);
        assert!(is_rule("---") && is_rule("* * *") && !is_rule("--") && !is_rule("-a-"));
    }
}
