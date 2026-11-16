//! エディタ用のMarkdownシンタックスハイライトと、カーソル位置まわりの補助関数。

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{Color32, FontFamily, FontId};
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
            }
        }
    }
}

struct Painter<'a> {
    job: LayoutJob,
    size: f32,
    c: &'a Colors,
}

impl Painter<'_> {
    fn fmt(&self, color: Color32) -> TextFormat {
        TextFormat {
            font_id: FontId::new(self.size, FontFamily::Proportional),
            color,
            ..Default::default()
        }
    }

    fn mono(&self, color: Color32, bg: bool) -> TextFormat {
        TextFormat {
            font_id: FontId::new(self.size * 0.93, FontFamily::Monospace),
            color,
            background: if bg { self.c.code_bg } else { Color32::TRANSPARENT },
            ..Default::default()
        }
    }

    fn push(&mut self, text: &str, format: TextFormat) {
        if !text.is_empty() {
            self.job.append(text, 0.0, format);
        }
    }

    /// 1行ぶんのインライン要素（コード・リンク・タグ・強調）に色を付ける。
    fn inline(&mut self, line: &str) {
        let b = line.as_bytes();
        let mut plain_from = 0;
        let mut i = 0;
        // 行頭のリスト記号・引用記号は薄く表示する
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let marker_len = if trimmed.starts_with("- [ ] ") || trimmed.starts_with("- [x] ") {
            6
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("> ")
        {
            2
        } else {
            let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && trimmed[digits..].starts_with(". ") {
                digits + 2
            } else {
                0
            }
        };
        if marker_len > 0 {
            let end = indent + marker_len;
            self.push(&line[..end], self.fmt(self.c.dim));
            plain_from = end;
            i = end;
        }

        while i < b.len() {
            let token: Option<(usize, TextFormat)> = match b[i] {
                b'`' => {
                    let n = b[i..].iter().take_while(|&&c| c == b'`').count();
                    line[i + n..]
                        .find(&line[i..i + n])
                        .map(|close| (i + n + close + n, self.mono(self.c.code, true)))
                }
                b'[' if b.get(i + 1) == Some(&b'[') => line[i + 2..]
                    .find("]]")
                    .map(|close| (i + 2 + close + 2, self.fmt(self.c.accent))),
                b'[' => line[i..].find("](").and_then(|mid| {
                    if line[i + 1..i + mid].contains('[') {
                        return None;
                    }
                    line[i + mid..]
                        .find(')')
                        .map(|close| (i + mid + close + 1, self.fmt(self.c.accent)))
                }),
                b'*' if b.get(i + 1) == Some(&b'*') => line[i + 2..]
                    .find("**")
                    .filter(|&close| close > 0)
                    .map(|close| (i + 2 + close + 2, self.fmt(self.c.strong))),
                b'#' if i == 0 || b[i - 1].is_ascii_whitespace() => {
                    let rest = &line[i + 1..];
                    let end = rest
                        .char_indices()
                        .find(|&(_, c)| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '/')))
                        .map_or(rest.len(), |(p, _)| p);
                    (end > 0 && !rest[..end].chars().all(|c| c.is_ascii_digit()))
                        .then(|| (i + 1 + end, self.fmt(self.c.tag)))
                }
                _ => None,
            };
            match token {
                Some((end, format)) => {
                    self.push(&line[plain_from..i], self.fmt(self.c.text));
                    self.push(&line[i..end], format);
                    plain_from = end;
                    i = end;
                }
                None => i += 1,
            }
        }
        self.push(&line[plain_from..], self.fmt(self.c.text));
    }
}

pub fn highlight(text: &str, size: f32, colors: &Colors) -> LayoutJob {
    let mut p = Painter {
        job: LayoutJob::default(),
        size,
        c: colors,
    };
    let mut in_frontmatter = text.starts_with("---\n") || text.starts_with("---\r\n");
    let mut fence: Option<(u8, usize)> = None;

    for (n, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.trim_end_matches(['\n', '\r']);
        let eol = &raw[line.len()..];
        let trimmed = line.trim_start();

        if in_frontmatter {
            p.push(raw, p.mono(p.c.dim, false));
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
        if let Some((ch, len)) = marker {
            match fence {
                None => fence = Some((ch, len)),
                Some((open, open_len)) if open == ch && len >= open_len => fence = None,
                _ => {}
            }
            p.push(raw, p.mono(p.c.dim, true));
            continue;
        }
        if fence.is_some() {
            p.push(raw, p.mono(p.c.code, true));
            continue;
        }

        let level = trimmed.bytes().take_while(|&c| c == b'#').count();
        let is_heading = (1..=6).contains(&level)
            && (trimmed.len() == level || trimmed[level..].starts_with(' '));
        if is_heading {
            let scale = [1.6, 1.4, 1.22, 1.1, 1.05, 1.0][level - 1];
            let mut format = p.fmt(p.c.heading);
            format.font_id.size = size * scale;
            let marker_end = line.len() - trimmed.len() + level;
            let mut dim = format.clone();
            dim.color = p.c.dim;
            p.push(&line[..marker_end], dim);
            p.push(&line[marker_end..], format);
        } else {
            p.inline(line);
        }
        p.push(eol, p.fmt(p.c.text));
    }
    p.job
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

    #[test]
    fn highlight_covers_all_text() {
        let t = "---\na: b\n---\n# H\n- [ ] `c` [[l]] **b** #tag [x](y)\n```\ncode\n```\ntail";
        let job = highlight(t, 14.0, &Colors::new(true));
        assert_eq!(job.text, t);
    }
}
