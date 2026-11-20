//! エディタ用のMarkdownシンタックスハイライトと、カーソル位置まわりの補助関数。

use crate::fonts;
use crate::highlight;
use eframe::egui::text::{LayoutJob, LayoutSection, TextFormat};
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
    /// タスクのチェックボックス（範囲は `[ ]` の3文字。中身は状態の文字: ' ' 未完了、'x' 完了、'/' '-' など）
    Checkbox(char),
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
    /// 埋め込んだ音声（範囲は行全体、中身はリンク先）。行の下に再生ボタンを描く
    Audio(String),
    /// コールアウトの1行（範囲は行全体）。タイトルが無い最初の行には、種類名を描く位置（文字位置）が付く
    Callout {
        color: Color32,
        first: bool,
        label: Option<(String, usize)>,
        /// 折りたためるコールアウトの最初の行: (行番号, いま開いているか)
        fold: Option<(usize, bool)>,
    },
    /// 表（範囲は表全体。先頭の行が見出し）
    Table(Vec<TableRow>),
    /// 数式。表示数式（`$$…$$`）は範囲がブロック全体、行内数式は幅を確保した1文字
    Math { tex: String, display: bool },
    /// 文中の画像（範囲は幅を確保した1文字。中身はリンク先と描く大きさ）
    InlineImage(String, Vec2),
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
    /// 折りたためるコールアウト（最初の行の番号）を、利用者が開いた / 閉じたか。決めていなければ None
    pub callout_open: &'a dyn Fn(usize) -> Option<bool>,
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
    image: &'a dyn Fn(&str) -> Option<Vec2>,
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
            if b[i] == b'!' && self.live {
                if let Some((len, target, size)) = inline_image(&line[i..])
                    .and_then(|(len, t)| (self.image)(&t).map(|s| (len, t, s)))
                {
                    self.push(&line[plain_from..i], self.fmt(base));
                    self.inline_image(&line[i..i + len], target, size);
                    plain_from = i + len;
                    i += len;
                    continue;
                }
            }
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

    /// 文中の画像。カーソルが触れていなければ、記法を畳んで画像の幅を確保し、上に画像を描く。
    /// 行の高さは画像に合わせ、記法を見せるときも同じにする。
    fn inline_image(&mut self, src: &str, target: String, size: Vec2) {
        // 大きな画像は、行が高くなりすぎないよう縮める
        let scale = (INLINE_IMAGE_MAX / size.y).min(1.0);
        let size = size * scale;
        let row = Some((self.size * LINE_HEIGHT).max(size.y + 4.0));
        if self.hidden(self.ahead(src)) {
            let mut slot = self.mark(true);
            slot.extra_letter_spacing = size.x + 2.0;
            slot.line_height = row;
            let at = self.push(&src[..1], slot);
            let mut rest = self.mark(true);
            rest.line_height = row;
            self.push(&src[1..], rest);
            self.deco(DecoKind::InlineImage(target, size), at);
        } else {
            let mut f = self.fmt(self.c.accent);
            f.line_height = row;
            self.push(src, f);
        }
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
        // タスク（箇条書き・番号付き・引用の中も）。判定は一覧（core::task）と同じものを使う
        if let Some((status_at, status, _)) = robsidian_core::task::task_in_line(line) {
            let open = status_at - 1;
            let close = status_at + status.len_utf8() + 1;
            let quoted = line[..open].trim_start().starts_with('>');
            let numbered = line[..open].trim_end().ends_with(['.', ')']);
            let hide = self.hidden(self.ahead(&line[..close]));
            if numbered {
                // 番号は畳まずに薄く見せる（Obsidian と同じ）
                self.push(&line[..open], self.fmt(self.c.dim));
            } else {
                let bullet = open - 2;
                let lead = &line[..bullet];
                if quoted {
                    self.push(lead, self.ghost(hide));
                } else {
                    self.push(lead, self.indent_fmt(lead));
                }
                self.push(&line[bullet..open], self.mark(hide));
            }
            let range = self.push(&line[open..close], self.ghost(hide));
            if hide {
                self.deco(DecoKind::Checkbox(status), range);
            }
            let body = if line[close..].starts_with(' ') {
                self.push(" ", self.fmt(text));
                close + 1
            } else {
                close
            };
            let color = if status == ' ' { text } else { self.c.dim };
            return (body, color, quoted);
        }
        if trimmed.starts_with("> ") || trimmed == ">" {
            let end = (indent + 2).min(line.len());
            let hide = self.hidden(self.ahead(&line[..end]));
            self.push(&line[..indent], self.indent_fmt(&line[..indent]));
            self.push(&line[indent..end], self.ghost(hide));
            return (end, self.c.quote, hide);
        }
        let bullet = ["- ", "* ", "+ "].iter().any(|m| trimmed.starts_with(m));
        if bullet {
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

/// 文中の画像の高さの上限
const INLINE_IMAGE_MAX: f32 = 200.0;

/// `![[画像]]` / `![代替](画像)` で始まっていれば、その長さ（バイト）とリンク先。
fn inline_image(s: &str) -> Option<(usize, String)> {
    if let Some(rest) = s.strip_prefix("![[") {
        let close = rest.find("]]")?;
        let inner = &rest[..close];
        let target = inner.split(['|', '#']).next()?.trim();
        if target.is_empty() || inner.contains('[') {
            return None;
        }
        return Some((3 + close + 2, target.to_string()));
    }
    let rest = s.strip_prefix("![")?;
    let mid = rest.find("](")?;
    if rest[..mid].contains(['[', ']']) {
        return None;
    }
    let close = rest[mid + 2..].find(')')?;
    let url = &rest[mid + 2..mid + 2 + close];
    if url.contains("://") || url.trim().is_empty() {
        return None;
    }
    let url = url.trim().trim_start_matches('<').trim_end_matches('>');
    Some((2 + mid + 2 + close + 1, parse::percent_decode(url)))
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
        // 表のセルの数式・画像は、記法を見せたまま色だけ付ける（上に描く仕組みが無いため）
        math: &|_: &str, _: bool| None,
        image: &|_: &str| None,
    };
    p.inline(text, color);
    p.job
}

/// 埋め込んだ音声の再生ボタンの行の高さ
pub const AUDIO_HEIGHT: f32 = 32.0;

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

/// 折りたためるコールアウトなら、最初に開いているか（`+` なら開、`-` なら閉）。
pub fn callout_fold(line: &str) -> Option<bool> {
    let (_, len) = callout_kind(line)?;
    let t = line.trim_start().strip_prefix('>')?.trim_start();
    match t.as_bytes().get(len - 1) {
        Some(b'+') => Some(true),
        Some(b'-') => Some(false),
        _ => None,
    }
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

/// 本文全体を一度に組む（[`BlockCache::layout`] の結果と同じになることをテストで確かめる）
#[cfg(test)]
pub fn layout(text: &str, opt: &Options) -> Layout {
    let mut out = layout_part(text, opt, true, 0);
    paint_marks(&mut out.job, opt.marks, opt.current_mark, opt.colors);
    out
}

/// 本文の一部（`line_base` 行目から）を組む。位置は `text` の先頭からの相対。
/// `frontmatter` は、`text` が本文の先頭で frontmatter を持ちうるか。検索の一致の色は付けない。
fn layout_part(text: &str, opt: &Options, frontmatter: bool, line_base: usize) -> Layout {
    let mut p = Painter {
        job: LayoutJob::default(),
        decos: Vec::new(),
        chars: 0,
        size: opt.size,
        c: opt.colors,
        live: opt.live,
        cursor: opt.cursor,
        math: opt.math,
        image: opt.image,
    };
    let size = opt.size;
    let mut in_frontmatter =
        frontmatter && (text.starts_with("---\n") || text.starts_with("---\r\n"));
    let mut fence: Option<(u8, usize)> = None;
    // 処理中の表: (最後の行, 整形表示するか, 表の開始文字位置, 集めた行) と、その最初の行
    let mut table: Option<(usize, bool, usize, Vec<TableRow>)> = None;
    let mut first_line = 0;
    // 処理中のコールアウト: (種類, 最初の行)
    // (種類, 最初の行, 本文を畳んでいるか, 折りたためるなら開いているか)
    let mut callout: Option<(String, usize, bool, Option<bool>)> = None;
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
        } else if let Some(target) = embed_only(line)
            .filter(|t| opt.live && crate::preview::audio_on() && crate::player::is_audio(t))
        {
            // 記法の行の下に、再生ボタンの場所を確保する
            let mut format = if hide { p.mark(true) } else { p.fmt(p.c.dim) };
            format.line_height = Some(line_height(size) + AUDIO_HEIGHT + 8.0);
            format.valign = Align::TOP;
            let range = p.push(line, format);
            p.deco(DecoKind::Audio(target), range);
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
                callout = callout_kind(line).map(|(kind, _)| {
                    // `[!種類]-` は閉じた状態、`+` は開いた状態で始まる折りたためるコールアウト
                    let open = callout_fold(line)
                        .map(|default| (opt.callout_open)(line_base + n).unwrap_or(default));
                    let end = (n + 1..lines.len())
                        .take_while(|&k| content(lines[k]).trim_start().starts_with('>'))
                        .last()
                        .unwrap_or(n);
                    let title = lines[n].chars().count();
                    let body: usize = lines[n + 1..=end].iter().map(|l| l.chars().count()).sum();
                    // 本文にカーソルがあるときは開いて見せる（見えない文字を編集しないように）
                    let collapsed = open == Some(false)
                        && end > n
                        && p.hidden(p.chars + title..p.chars + title + body);
                    (kind, n, collapsed, open)
                });
            }
            if let Some((_, first, true, _)) = &callout {
                if *first != n {
                    // 畳んだ本文の行は、高さ0で見えなくする
                    let mut gone = p.mark(true);
                    gone.line_height = Some(0.0);
                    p.push(line, gone.clone());
                    p.push(eol, gone);
                    continue;
                }
            }
            let (body, color, quote_bar) = p.block_prefix(line);
            match callout.clone() {
                Some((kind, first, collapsed, open)) => {
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
                        // 完了したタスクは薄く、それ以外は本文の色（引用の色にはしない）
                        let color = if color == p.c.dim { color } else { p.c.text };
                        p.inline(&line[body..], color);
                    }
                    let fold = (first == n)
                        .then_some(open)
                        .flatten()
                        .map(|_| (line_base + n, !collapsed));
                    p.deco(
                        DecoKind::Callout {
                            color: accent,
                            first: first == n,
                            label,
                            fold,
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
    Layout {
        job: p.job,
        decos: p.decos,
    }
}

/// 長いノートの組版を速くするため、本文をブロック（ふつうは1行）に分けて組み、前回の組版のうち
/// 変わったブロックだけを組み直して差し替える。カーソルが触れているブロックも組み直す
/// （記法を見せる範囲が変わるので）。
///
/// 文字サイズ・テーマ・幅・画像の大きさなどが変わったら [`BlockCache::clear`] で捨てる。
/// カーソルと検索の一致は、ここで扱う。
#[derive(Default)]
pub struct BlockCache {
    /// 前回組んだ本文
    text: String,
    /// 組んだもの（検索の一致の色は付けていない）
    job: LayoutJob,
    decos: Vec<Deco>,
    blocks: Vec<Block>,
    /// 検索の一致の色を付けたもの（一致があるときだけ）
    marked: Option<LayoutJob>,
    /// 次に必ず組み直す
    dirty: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Block {
    bytes: Range<usize>,
    chars: Range<usize>,
    line_base: usize,
    /// ブロックの中でのカーソルの位置（触れていなければ None）
    cursor: Option<(usize, usize)>,
    /// 折りたためるコールアウトを含む（行番号が変わったら組み直す）
    folds: bool,
    /// 毎回組み直す（[`blocks`]）
    volatile: bool,
    /// `job.sections` と `decos` の中の、このブロックの分
    sections: Range<usize>,
    decos: Range<usize>,
}

/// 本文の `from` バイト目（`line0` 行目の先頭）からをブロックに分ける:
/// (バイト範囲, 最初の行の番号, 毎回組み直すか)。ふつうは1行が1ブロック。
/// `from` が 0 でなければ、ブロックの区切り（組版の状態が空の所）であること。
/// 区切りの位置ごとに `stop(バイト位置, 行)` を呼び、true なら、そこで止めて区切りの位置を返す。
///
/// 組版（[`layout_part`]）が複数の行をまとめて扱うもの（frontmatter・コードブロック・表示数式・
/// 表・コールアウトや引用）の途中では分けない。そのため [`layout_part`] と同じ順に行を見分けて、
/// 状態を多めに見積もって追う（見積もりが外れても、ブロックが大きくなるだけ）。
/// 閉じの無い `$$` の行は、後ろの行（ブロックの外）を書き換えると数式になりうるので、毎回組み直す。
type Split = (Range<usize>, usize, bool);

fn blocks(
    text: &str,
    from: usize,
    line0: usize,
    mut stop: impl FnMut(usize, usize) -> bool,
) -> (Vec<Split>, Option<(usize, usize)>) {
    let lines: Vec<&str> = text[from..].split_inclusive('\n').collect();
    let mut out = Vec::new();
    let mut start = from;
    let mut first = line0;
    let mut volatile = false;
    let mut at = from;
    let mut frontmatter = from == 0 && (text.starts_with("---\n") || text.starts_with("---\r\n"));
    let mut fence: Option<(u8, usize)> = None;
    // 表示数式の最後の行（組版では、最後の行を過ぎても次に数式の分岐に来る行までは残る）
    let mut math: Option<usize> = None;
    // コールアウト（引用）の中かもしれない
    let mut callout = false;
    // 前の行が `|` で始まる（表かどうかを、次の行で決める）
    let mut prev_pipe = false;
    for (n, raw) in lines.iter().enumerate() {
        let line = raw.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim_start();
        if n > 0 && !frontmatter && fence.is_none() && math.is_none() && !callout && !prev_pipe {
            out.push((start..at, first, volatile));
            if stop(at, line0 + n) {
                return (out, Some((at, line0 + n)));
            }
            start = at;
            first = line0 + n;
            volatile = false;
        }
        prev_pipe = trimmed.starts_with('|');
        at += raw.len();
        if frontmatter {
            if n > 0 && matches!(line.trim_end(), "---" | "...") {
                frontmatter = false;
            }
            continue;
        }
        // コードブロック
        let marker = {
            let b = trimmed.as_bytes();
            b.first()
                .filter(|&&c| c == b'`' || c == b'~')
                .map(|&c| (c, b.iter().take_while(|&&x| x == c).count()))
                .filter(|&(_, len)| len >= 3)
        };
        if fence.is_some() || marker.is_some() {
            fence = match (fence, marker) {
                (Some((oc, ol)), Some((c, l))) if c == oc && l >= ol => None,
                (Some(f), _) => Some(f),
                (None, m) => m,
            };
            continue;
        }
        // 見出し・水平線・画像や埋め込みだけの行は、数式・表・引用の分岐に来ない
        let level = trimmed.bytes().take_while(|&c| c == b'#').count();
        let heading = (1..=6).contains(&level)
            && (trimmed.len() == level || trimmed[level..].starts_with(' '));
        if heading || is_rule(trimmed) || image_only(line).is_some() || embed_only(line).is_some() {
            continue;
        }
        // 表示数式。終わったと見積もった行が `$$` で始まるなら、組版ではそこで次の数式が
        // 始まっているかもしれない（上で多めに飛ばした行で、組版の数式はもう終わっていることがある）
        if let Some(end) = math {
            if n < end {
                continue;
            }
            math = None;
            if !trimmed.starts_with("$$") {
                continue;
            }
        }
        if trimmed.starts_with("$$") {
            match display_math(&lines, n) {
                Some((end, _)) => {
                    if n < end {
                        math = Some(end);
                    }
                    continue;
                }
                None => volatile = true,
            }
        }
        // 表は `|` の行が続く間（分けるかどうかは prev_pipe で決める）
        if trimmed.starts_with('|') {
            continue;
        }
        callout = trimmed.starts_with('>');
    }
    out.push((start..text.len(), first, volatile));
    (out, None)
}

/// 先頭から同じバイト数
fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    let n = a.len().min(b.len());
    let mut i = 0;
    // 64バイトずつ比べてから、1バイトずつ
    while i + 64 <= n && a[i..i + 64] == b[i..i + 64] {
        i += 64;
    }
    while i < n && a[i] == b[i] {
        i += 1;
    }
    i
}

/// 末尾から同じバイト数（`max` まで）
fn common_suffix(a: &[u8], b: &[u8], max: usize) -> usize {
    let (la, lb) = (a.len(), b.len());
    let mut i = 0;
    while i + 64 <= max && a[la - i - 64..la - i] == b[lb - i - 64..lb - i] {
        i += 64;
    }
    while i < max && a[la - i - 1] == b[lb - i - 1] {
        i += 1;
    }
    i
}

impl Deco {
    fn shift(&mut self, by: isize) {
        let mv = |x: usize| (x as isize + by) as usize;
        self.chars = mv(self.chars.start)..mv(self.chars.end);
        match &mut self.kind {
            DecoKind::Table(rows) => {
                for r in rows {
                    r.chars = mv(r.chars.start)..mv(r.chars.end);
                }
            }
            DecoKind::Callout {
                label: Some((_, at)),
                ..
            } => *at = mv(*at),
            _ => {}
        }
    }
}

impl BlockCache {
    /// 覚えている組版を捨てる
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// 次の [`Self::update`] で、本文が同じでも組み直す（カーソルや検索の一致が変わったとき）
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// `text` を組み直す必要があるか
    pub fn needs(&self, text: &str) -> bool {
        self.dirty || self.text != text || (self.job.sections.is_empty() && !text.is_empty())
    }

    /// 組んだもの（検索の一致の色を含む）
    pub fn job(&self) -> &LayoutJob {
        self.marked.as_ref().unwrap_or(&self.job)
    }

    pub fn decos(&self) -> &[Deco] {
        &self.decos
    }

    /// 本文全体を組んだもの（[`layout`] と同じ）にする。前回と同じブロックは組み直さない
    pub fn update(&mut self, text: &str, opt: &Options) {
        self.dirty = false;
        let old = std::mem::take(&mut self.text);
        let olds = std::mem::take(&mut self.blocks);
        let prefix = common_prefix(old.as_bytes(), text.as_bytes());
        let suffix = common_suffix(
            old.as_bytes(),
            text.as_bytes(),
            old.len().min(text.len()) - prefix,
        );
        let delta = text.len() as isize - old.len() as isize;
        let old_chars = olds.last().map_or(0, |b| b.chars.end);

        // 1. ブロックの区切り。変わった所より前で終わるブロックと、変わった所より後ろで前回の区切りに
        //    出会ってからのブロックは、前回の区切りを使う（区切りでは組版の状態が空なので、その後ろが同じなら
        //    区切りも同じ）。閉じの無い `$$` を含むブロックと最後のブロックは、後ろ次第で変わりうるので区切り直す
        let unchanged = !olds.is_empty() && old == text;
        let keep = if unchanged {
            olds.len()
        } else {
            olds.iter()
                .position(|b| b.bytes.end > prefix || b.volatile)
                .unwrap_or(olds.len())
                .min(olds.len().saturating_sub(1))
        };
        let (from, line) = olds
            .get(keep)
            .map_or((0, 0), |b| (b.bytes.start, b.line_base));
        let suffix_start = text.len() - suffix;
        let mut synced: Option<usize> = None;
        let (scanned, sync) = if unchanged {
            (Vec::new(), None)
        } else {
            blocks(text, from, line, |pos, _| {
                if pos < suffix_start || pos <= from {
                    return false;
                }
                let old_pos = (pos as isize - delta) as usize;
                let i = olds[keep..].partition_point(|b| b.bytes.start < old_pos) + keep;
                let hit = olds.get(i).is_some_and(|b| b.bytes.start == old_pos);
                if hit {
                    synced = Some(i);
                }
                hit
            })
        };
        // 区切り直した部分の後ろ（前回のブロック synced.. を動かして使う）
        let tail_from = synced.unwrap_or(olds.len());
        let line_delta = sync.map_or(0, |(_, l)| l as isize - olds[tail_from].line_base as isize);
        let mut chars = olds.get(keep).map_or(0, |b| b.chars.start);
        let scanned_from = chars;
        let mut new: Vec<Block> = Vec::with_capacity(olds.len() + scanned.len());
        new.extend(olds[..keep].iter().cloned());
        for (bytes, line_base, volatile) in scanned {
            let len = text[bytes.clone()].chars().count();
            new.push(Block {
                bytes,
                chars: chars..chars + len,
                line_base,
                cursor: None,
                folds: false,
                volatile,
                sections: 0..0,
                decos: 0..0,
            });
            chars += len;
        }
        let old_scanned_chars =
            olds.get(tail_from).map_or(old_chars, |b| b.chars.start) - scanned_from;
        let char_delta = (chars - scanned_from) as isize - old_scanned_chars as isize;
        let first_tail = new.len();
        let mv = |x: usize, by: isize| (x as isize + by) as usize;
        for o in &olds[tail_from..] {
            let mut b = o.clone();
            b.bytes = mv(o.bytes.start, delta)..mv(o.bytes.end, delta);
            b.chars = mv(o.chars.start, char_delta)..mv(o.chars.end, char_delta);
            b.line_base = mv(o.line_base, line_delta);
            new.push(b);
        }
        // 前回のブロックとの対応（区切り直したブロックは無し）
        let old_of = |i: usize| -> Option<usize> {
            if i < keep {
                Some(i)
            } else if i >= first_tail {
                Some(tail_from + i - first_tail)
            } else {
                None
            }
        };
        for b in &mut new {
            b.cursor = opt.cursor.and_then(|(x, y)| {
                (x <= b.chars.end && b.chars.start <= y)
                    .then(|| (x.saturating_sub(b.chars.start), y - b.chars.start))
            });
        }
        // 2. 前回の組版をそのまま使えるか（カーソルの位置が同じで、折りたたみの行番号も変わらない）
        let reusable = |i: usize, b: &Block| {
            old_of(i).is_some_and(|j| {
                let o = &olds[j];
                !o.volatile && o.cursor == b.cursor && (!o.folds || o.line_base == b.line_base)
            })
        };
        let dirty: Vec<usize> = (0..new.len()).filter(|&i| !reusable(i, &new[i])).collect();
        // 組み直す範囲（新しいブロックの番号）。区切り直したブロックは必ず含める
        let mut r0 = dirty.first().copied().unwrap_or(new.len()).min(keep);
        let mut r1 = dirty.last().map_or(0, |&i| i + 1).max(first_tail);
        if dirty.is_empty() && keep == first_tail {
            // 区切りも組版も変わらない（本文の長さが変わらず、カーソルも変わらない）
            r0 = keep;
            r1 = keep;
        }
        // 前回のブロックでの範囲
        let o0 = r0;
        let o1 = if r1 >= first_tail {
            tail_from + (r1 - first_tail)
        } else {
            r1
        };
        let sec_at = |j: usize| -> usize {
            olds.get(j)
                .map_or(self.job.sections.len(), |b| b.sections.start)
        };
        let deco_at =
            |j: usize| -> usize { olds.get(j).map_or(self.decos.len(), |b| b.decos.start) };
        let (sec_from, sec_to) = (sec_at(o0), sec_at(o1));
        let (deco_from, deco_to) = (deco_at(o0), deco_at(o1));

        let mut sections: Vec<LayoutSection> = Vec::new();
        let mut decos: Vec<Deco> = Vec::new();
        for (i, n) in new.iter_mut().enumerate().take(r1).skip(r0) {
            let s0 = sec_from + sections.len();
            let d0 = deco_from + decos.len();
            match old_of(i).filter(|_| !dirty.contains(&i)) {
                Some(j) => {
                    let o = &olds[j];
                    let shift = n.bytes.start as isize - o.bytes.start as isize;
                    let cshift = n.chars.start as isize - o.chars.start as isize;
                    for sec in &self.job.sections[o.sections.clone()] {
                        let mut sec = sec.clone();
                        sec.byte_range = mv(sec.byte_range.start.0, shift).into()
                            ..mv(sec.byte_range.end.0, shift).into();
                        sections.push(sec);
                    }
                    for d in &self.decos[o.decos.clone()] {
                        let mut d = d.clone();
                        d.shift(cshift);
                        decos.push(d);
                    }
                }
                None => {
                    let sub = Options {
                        cursor: n.cursor,
                        ..*opt
                    };
                    let part = layout_part(
                        &text[n.bytes.clone()],
                        &sub,
                        n.bytes.start == 0,
                        n.line_base,
                    );
                    let off = n.bytes.start;
                    sections.extend(part.job.sections.into_iter().map(|mut sec| {
                        sec.byte_range = (sec.byte_range.start.0 + off).into()
                            ..(sec.byte_range.end.0 + off).into();
                        sec
                    }));
                    n.folds = part
                        .decos
                        .iter()
                        .any(|d| matches!(d.kind, DecoKind::Callout { fold: Some(_), .. }));
                    decos.extend(part.decos.into_iter().map(|mut d| {
                        d.shift(n.chars.start as isize);
                        d
                    }));
                }
            }
            n.sections = s0..sec_from + sections.len();
            n.decos = d0..deco_from + decos.len();
        }

        // 3. 前回の組版の範囲を差し替え、後ろを動かす
        let sec_shift = sections.len() as isize - (sec_to - sec_from) as isize;
        let deco_shift = decos.len() as isize - (deco_to - deco_from) as isize;
        let sec_after = sec_from + sections.len();
        let deco_after = deco_from + decos.len();
        self.job.sections.splice(sec_from..sec_to, sections);
        self.decos.splice(deco_from..deco_to, decos);
        if delta != 0 {
            for sec in &mut self.job.sections[sec_after..] {
                sec.byte_range = mv(sec.byte_range.start.0, delta).into()
                    ..mv(sec.byte_range.end.0, delta).into();
            }
        }
        if char_delta != 0 {
            for d in &mut self.decos[deco_after..] {
                d.shift(char_delta);
            }
        }
        for n in &mut new[r1..] {
            n.sections = mv(n.sections.start, sec_shift)..mv(n.sections.end, sec_shift);
            n.decos = mv(n.decos.start, deco_shift)..mv(n.decos.end, deco_shift);
        }
        self.blocks = new;
        self.job.text.clear();
        self.job.text.push_str(text);
        self.text = old;
        self.text.clear();
        self.text.push_str(text);
        self.marked = (!opt.marks.is_empty()).then(|| {
            let mut job = self.job.clone();
            paint_marks(&mut job, opt.marks, opt.current_mark, opt.colors);
            job
        });
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

/// 文字の位置（何文字目か）から、組み立てた文の上の位置を引く表。
///
/// egui の `Galley::pos_from_cursor` は先頭の行から順にたどるので、長いノートで装飾ごとに呼ぶと遅い
/// （行数 × 装飾の数）。行ごとの先頭の文字位置を覚えておき、二分探索で引く。
pub struct RowIndex {
    /// 各行の先頭の文字位置
    starts: Vec<usize>,
}

impl RowIndex {
    pub fn new(galley: &egui::Galley) -> Self {
        let mut at = 0;
        let starts = galley
            .rows
            .iter()
            .map(|row| {
                let start = at;
                at += row.char_count_including_newline().0;
                start
            })
            .collect();
        Self { starts }
    }

    /// `galley.pos_from_cursor(CCursor::new(ch))` と同じ結果
    pub fn pos(&self, galley: &egui::Galley, ch: usize) -> egui::Rect {
        use egui::epaint::text::cursor::LayoutCursor;
        if self.starts.is_empty() {
            return galley.pos_from_cursor(egui::text::CCursor::new(ch));
        }
        let mut row = self.starts.partition_point(|&s| s <= ch).saturating_sub(1);
        // 折り返した行の境目（改行で終わらない行の末尾）は、egui と同じく前の行の末尾とみなす
        if row > 0 && ch == self.starts[row] && !galley.rows[row - 1].ends_with_newline {
            row -= 1;
        }
        let column = ch - self.starts[row];
        galley.pos_from_layout_cursor(&LayoutCursor {
            row,
            column: egui::text::CharIndex(column),
        })
    }
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
                callout_open: &|_: usize| None,
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

    fn options_for_test<'a>(
        colors: &'a Colors,
        cursor: Option<(usize, usize)>,
        callout_open: &'a dyn Fn(usize) -> Option<bool>,
    ) -> Options<'a> {
        Options {
            marks: &[],
            current_mark: None,
            size: 14.0,
            colors,
            live: true,
            cursor,
            image: &|t: &str| (t == "pic.png").then_some(Vec2::new(100.0, 50.0)),
            note_embed: &|_: &str| None,
            callout_open,
            math: &|tex: &str, display: bool| {
                Some(Vec2::new(
                    tex.len() as f32 * 7.0,
                    if display { 40.0 } else { 18.0 },
                ))
            },
            hide_frontmatter: true,
        }
    }

    #[test]
    fn block_cache_matches_full_layout() {
        let doc = "---\na: b\n\nc: d\n---\n# 見出し\n\n本文 **太字** [[リンク]]\n続き\n\n```rust\nfn a() {}\n\nfn b() {}\n```\n\n$$\nx\n\ny\n$$\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> [!note]- 畳む\n> 中身\n\n> [!tip]\n> 本文\n\n- [ ] タスク\n- 項目\n\n---\n\n![[pic.png]]\n\n```\n閉じない\n\nコード";
        // 空行を挟まずに書いたもの（表やコールアウトのすぐ後に本文、見出しの連続など）
        let tight = "# 一\n## 二\n本文\n| a | b |\n|---|---|\n| 1 | 2 |\n続き\n> [!note]+ 開く\n> 中\n後\n> 引用\n- [x] 済\n```\nx\n```\n$$\nz\n$$\n終わり $y$ **b**\n";
        for doc in [doc, tight] {
            for live in [true, false] {
                check_block_cache(doc, live);
            }
        }
    }

    fn check_block_cache(doc: &str, live: bool) {
        let colors = Colors::new(true);
        let folds = |line: usize| (line == 30 || line == 7).then_some(true);
        let mut cache = BlockCache::default();
        let mut text = doc.to_string();
        let total = |t: &str| t.chars().count();
        // 編集とカーソルの移動を繰り返して、毎回全体を組んだものと比べる
        // (カーソル, 編集: (文字位置, そこの1文字と置き換える文字列))
        type Step<'a> = (Option<(usize, usize)>, Option<(usize, &'a str)>);
        let mut steps: Vec<Step> = vec![(None, None)];
        for at in [0, 5, 12, 20, 40, 60, 80, 100, 120, 150] {
            steps.push((Some((at, at)), None));
            steps.push((Some((at, at + 3)), Some((at, "x\n\n"))));
            steps.push((Some((at, at)), Some((at, ""))));
        }
        // 乱数（線形合同法）で、記法の文字を入れたり消したり、カーソルを飛ばしたりする
        let pieces = [
            "",
            "a",
            "\n",
            "\n\n",
            "```",
            "~~~\n",
            "| x |\n",
            "|---|\n",
            "> ",
            "> [!tip]-\n",
            "$$",
            "$$\n",
            "---\n",
            "# ",
            "- [ ] ",
            "**",
            "[[",
            "]]",
            "あ",
            "\t",
            "![[pic.png]]\n",
            "```rust\n",
            "$x$",
            "***\n",
        ];
        let mut seed: u64 = 12345;
        let mut rand = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n.max(1)
        };
        for _ in 0..400 {
            let at = rand(doc.chars().count() + 20);
            let cursor = match rand(4) {
                0 => None,
                1 => Some((at, at)),
                _ => Some((at, at + rand(30))),
            };
            let edit =
                (rand(3) > 0).then(|| (rand(doc.chars().count() + 20), pieces[rand(pieces.len())]));
            steps.push((cursor, edit));
        }
        let mut prev = String::new();
        let mut prev_cursor = None;
        for (cursor, edit) in steps {
            if let Some((at, insert)) = edit {
                let n = total(&text);
                if at < n {
                    let b = byte_of_char(&text, at);
                    let e = byte_of_char(&text, (at + 1).min(n));
                    text.replace_range(b..e, insert);
                }
            }
            let cursor = cursor.map(|(a, b)| (a.min(total(&text)), b.min(total(&text))));
            let opt = Options {
                live,
                ..options_for_test(&colors, cursor, &folds)
            };
            let full = layout(&text, &opt);
            cache.update(&text, &opt);
            let fast = Layout {
                job: cache.job().clone(),
                decos: cache.decos().to_vec(),
            };
            assert_eq!(fast.job.text, full.job.text);
            // ブロックの継ぎ目で同じ書式の区間が分かれていることがある（描いた結果は同じ）ので、つなげて比べる
            let merged = |l: &Layout| {
                let mut out: Vec<LayoutSection> = Vec::new();
                for sec in &l.job.sections {
                    match out.last_mut() {
                        Some(last)
                            if sec.leading_space == 0.0
                                && last.format == sec.format
                                && last.byte_range.end == sec.byte_range.start =>
                        {
                            last.byte_range.end = sec.byte_range.end;
                        }
                        _ => out.push(sec.clone()),
                    }
                }
                out
            };
            let (a, b) = (merged(&fast), merged(&full));
            if let Some(i) = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
                let show = |v: &[LayoutSection]| {
                    v.get(i).map(|s| {
                        format!(
                            "{:?} {:?}",
                            s.byte_range,
                            &text[s.byte_range.start.0..s.byte_range.end.0],
                        )
                    })
                };
                panic!(
                    "{cursor:?} {i}: {:?} != {:?}\nOLD {prev_cursor:?} {prev:?}\nNEW {text:?}",
                    show(&a),
                    show(&b)
                );
            }
            assert_eq!(fast.decos, full.decos, "{cursor:?} {text:?}");
            prev = text.clone();
            prev_cursor = cursor;
        }
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
                DecoKind::Checkbox(' '),
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
                callout_open: &|_: usize| None,
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
                callout_open: &|_: usize| None,
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
        let text =
            "> [!tip]- Title\n> body\nafter\n> [!note]\n> b\n![[Other#h]]\n![[rec.wav]]\nend";
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
                callout_open: &|_: usize| None,
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
                DecoKind::Audio(t) => format!("audio:{t}"),
                other => format!("{other:?}"),
            })
            .collect();
        // `[!tip]-` は閉じた状態で始まるので、本文の行の装飾は無い
        assert_eq!(
            kinds,
            [
                "callout:true:-",
                "callout:true:Note",
                "callout:false:-",
                "embed:Other#h",
                "audio:rec.wav"
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
    fn inline_images_reserve_space() {
        let text = "前 ![[pic.png]] 後 ![[nope.png]] ![a](pic.png)";
        let out = run(text, true, None);
        assert_eq!(out.job.text, text);
        let images: Vec<_> = out
            .decos
            .iter()
            .filter_map(|d| match &d.kind {
                DecoKind::InlineImage(t, size) => Some((t.clone(), *size)),
                _ => None,
            })
            .collect();
        // 読めない画像（nope.png）は記法のまま
        assert_eq!(
            images,
            [
                ("pic.png".to_string(), Vec2::new(100.0, 50.0)),
                ("pic.png".to_string(), Vec2::new(100.0, 50.0))
            ]
        );
        // カーソルが触れている画像は記法を見せる
        assert_eq!(run(text, true, Some((3, 3))).decos.len(), 1);
        assert_eq!(
            inline_image("![[a b.png|200]] x"),
            Some((16, "a b.png".into()))
        );
        assert_eq!(
            inline_image("![x](img/a%20b.png) y"),
            Some((19, "img/a b.png".into()))
        );
        assert_eq!(inline_image("![x](https://e/x.png)"), None);
        assert_eq!(inline_image("![[]]"), None);
    }

    #[test]
    fn foldable_callouts() {
        let colors = Colors::new(true);
        let text =
            "> [!tip]- 閉じている\n> 本文1\n> 本文2\nafter\n> [!note]+ 開いている\n> 本文3\n";
        let image = |_: &str| None;
        let run_with = |cursor: Option<(usize, usize)>, opened: Option<bool>| {
            let open = move |line: usize| (line == 0).then_some(opened).flatten();
            layout(
                text,
                &Options {
                    marks: &[],
                    current_mark: None,
                    size: 14.0,
                    colors: &colors,
                    live: true,
                    cursor,
                    image: &image,
                    note_embed: &|_: &str| None,
                    callout_open: &open,
                    math: &|_: &str, _: bool| None,
                    hide_frontmatter: false,
                },
            )
        };
        // 畳んだ本文の行は高さ0
        let collapsed = |out: &Layout| -> String {
            out.job
                .sections
                .iter()
                .filter(|s| s.format.line_height == Some(0.0))
                .map(|s| &text[s.byte_range.start.0..s.byte_range.end.0])
                .collect()
        };
        let folds = |out: &Layout| -> Vec<(usize, bool)> {
            out.decos
                .iter()
                .filter_map(|d| match &d.kind {
                    DecoKind::Callout { fold, .. } => *fold,
                    _ => None,
                })
                .collect()
        };
        let out = run_with(None, None);
        assert_eq!(out.job.text, text);
        assert_eq!(collapsed(&out), "> 本文1\n> 本文2\n");
        assert_eq!(folds(&out), [(0, false), (4, true)]);
        // 利用者が開いた
        let out = run_with(None, Some(true));
        assert_eq!(collapsed(&out), "");
        assert_eq!(folds(&out), [(0, true), (4, true)]);
        // 本文にカーソルがあるときは開いて見せる
        let out = run_with(Some((18, 18)), None);
        assert_eq!(collapsed(&out), "");
        // タイトルにカーソルがあっても畳んだまま
        let out = run_with(Some((3, 3)), None);
        assert_eq!(collapsed(&out), "> 本文1\n> 本文2\n");
        assert_eq!(callout_fold("> [!tip]- x"), Some(false));
        assert_eq!(callout_fold("> [!TIP]+"), Some(true));
        assert_eq!(callout_fold("> [!tip] x"), None);
    }

    #[test]
    fn tasks_in_all_forms_get_checkboxes() {
        let text =
            "- [ ] a\n- [x] b\n1. [ ] c\n2) [/] d\n> - [-] e\n- [ ]\n  * [>] f\n- [ab] no\n-[ ] no";
        let out = run(text, true, None);
        assert_eq!(out.job.text, text);
        let boxes: Vec<char> = out
            .decos
            .iter()
            .filter_map(|d| match d.kind {
                DecoKind::Checkbox(c) => Some(c),
                _ => None,
            })
            .collect();
        assert_eq!(boxes, [' ', 'x', ' ', '/', '-', ' ', '>']);
        // チェックボックスは `[ ]` の3文字の上に描く
        for d in out
            .decos
            .iter()
            .filter(|d| matches!(d.kind, DecoKind::Checkbox(_)))
        {
            let chars: String = text
                .chars()
                .skip(d.chars.start)
                .take(d.chars.len())
                .collect();
            assert!(chars.starts_with('[') && chars.ends_with(']'), "{chars}");
        }
        // 引用の中のタスクは引用の縦線も描く
        assert!(out.decos.iter().any(|d| d.kind == DecoKind::Quote));
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

    #[test]
    fn row_index_matches_egui() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            // 折り返しのある行・空の行・改行で終わる行を混ぜる
            let text = "短い\n折り返すとても長い行です。折り返すとても長い行です。折り返す\n\n最後";
            let galley = ui.fonts_mut(|f| {
                f.layout(
                    text.to_string(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                    60.0,
                )
            });
            assert!(galley.rows.len() > 4, "折り返していない");
            let index = RowIndex::new(&galley);
            for ch in 0..=text.chars().count() + 1 {
                assert_eq!(
                    index.pos(&galley, ch),
                    galley.pos_from_cursor(egui::text::CCursor::new(ch)),
                    "{ch} 文字目"
                );
            }
        });
        // テストでは描かないので、フォントのテクスチャの更新は捨てる
        out.textures_delta.clear();
    }
}
