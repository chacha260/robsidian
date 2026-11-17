//! Markdownノートの軽量パーサ。
//!
//! 完全なCommonMark解析ではなく、索引に必要な情報（見出し・リンク・タグ・
//! frontmatter）だけを1パスで拾う。数万ノートを起動時に舐めるので速度優先。

use serde::Serialize;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    /// 0始まりの行番号
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LinkKind {
    /// `[[target]]`
    Wiki,
    /// `[text](target.md)`
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    pub kind: LinkKind,
    /// リンク先（`#見出し` と `|別名` を除いた部分。Markdownリンクはデコード済み）
    pub target: String,
    pub heading: Option<String>,
    pub alias: Option<String>,
    /// `![[...]]` / `![](...)` の埋め込みか
    pub embed: bool,
    pub line: usize,
    /// ソース中で `target` が書かれているバイト範囲（リネーム時の書き換え用）
    #[serde(skip)]
    pub span: Range<usize>,
    /// Wikiリンクでは `[[...]]`（埋め込みなら先頭の `!` も含む）全体、
    /// Markdownリンクでは `span` と同じ範囲
    #[serde(skip)]
    pub full: Range<usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Parsed {
    pub headings: Vec<Heading>,
    pub links: Vec<Link>,
    /// `#` を除いたタグ（本文とfrontmatterの両方、重複なし、出現順）
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    /// frontmatterを除いた本文の開始バイト位置
    pub body_start: usize,
    /// タスク（`- [ ] ...`）。コードブロックの中は含まない
    pub tasks: Vec<crate::task::Task>,
    /// 本文中の `#タグ` のタグ名（`#` を除く）のバイト範囲。frontmatterのタグは含まない
    #[serde(skip)]
    pub tag_spans: Vec<Range<usize>>,
}

pub fn parse(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut offset = 0usize;
    let mut line_no = 0usize;

    if let Some(end) = frontmatter_end(text) {
        parse_frontmatter(&text[..end], &mut out);
        out.body_start = end;
        line_no = text[..end].matches('\n').count();
        offset = end;
    }

    let mut fence: Option<(u8, usize)> = None;
    for raw in text[offset..].split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if let Some((ch, len)) = fence_marker(trimmed) {
            match fence {
                None => fence = Some((ch, len)),
                Some((open_ch, open_len)) if ch == open_ch && len >= open_len => fence = None,
                _ => {}
            }
        } else if fence.is_none() {
            if indent <= 3 {
                if let Some(h) = parse_heading(trimmed, line_no) {
                    out.headings.push(h);
                }
            }
            scan_inline(line, offset, line_no, &mut out);
            if let Some((at, status, body)) = crate::task::task_in_line(line) {
                let (text, due) = crate::task::split_due(body);
                out.tasks.push(crate::task::Task {
                    line: line_no,
                    status,
                    text,
                    due,
                    marker: offset + at..offset + at + status.len_utf8(),
                });
            }
        }

        offset += raw.len();
        line_no += 1;
    }
    out
}

/// 先頭が `---` で始まるYAML frontmatterの終端（閉じ `---` 行の直後）を返す。
fn frontmatter_end(text: &str) -> Option<usize> {
    let first = text.split_inclusive('\n').next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let mut pos = first.len();
    for raw in text[pos..].split_inclusive('\n') {
        pos += raw.len();
        let t = raw.trim_end();
        if t == "---" || t == "..." {
            return Some(pos);
        }
    }
    None
}

/// `tags` / `aliases` だけを読む最小限のYAML解釈。
fn parse_frontmatter(fm: &str, out: &mut Parsed) {
    let mut current: Option<&str> = None;
    for line in fm.lines().skip(1) {
        if line.trim() == "---" {
            break;
        }
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            if line.starts_with([' ', '\t', '-']) {
                if let Some(key) = current {
                    push_fm_value(key, item, out);
                }
                continue;
            }
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            current = None;
            continue;
        };
        let key = key.trim();
        current = match key {
            "tags" | "tag" => Some("tags"),
            "aliases" | "alias" => Some("aliases"),
            _ => None,
        };
        let Some(key) = current else { continue };
        let value = value.trim();
        let inner = value
            .strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .unwrap_or(value);
        for v in inner.split(',') {
            push_fm_value(key, v, out);
        }
    }
}

fn push_fm_value(key: &str, raw: &str, out: &mut Parsed) {
    let v = raw.trim().trim_matches(['"', '\'']).trim();
    if v.is_empty() {
        return;
    }
    if key == "tags" {
        // `tags: a b` のような空白区切りも許容する
        for t in v.split_whitespace() {
            push_unique(&mut out.tags, t.trim_start_matches('#'));
        }
    } else {
        push_unique(&mut out.aliases, v);
    }
}

fn push_unique(list: &mut Vec<String>, v: &str) {
    if !v.is_empty() && !list.iter().any(|x| x == v) {
        list.push(v.to_string());
    }
}

fn fence_marker(trimmed: &str) -> Option<(u8, usize)> {
    let b = trimmed.as_bytes();
    let ch = *b.first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let len = b.iter().take_while(|&&c| c == ch).count();
    (len >= 3).then_some((ch, len))
}

fn parse_heading(trimmed: &str, line: usize) -> Option<Heading> {
    let level = trimmed.bytes().take_while(|&c| c == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim_end();
    Some(Heading {
        level: level as u8,
        text: text.to_string(),
        line,
    })
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

fn scan_inline(line: &str, base: usize, line_no: usize, out: &mut Parsed) {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'`' => {
                // インラインコード: 同じ長さのバッククォート列までを読み飛ばす
                let n = b[i..].iter().take_while(|&&c| c == b'`').count();
                let ticks = &line[i..i + n];
                if let Some(close) = line[i + n..].find(ticks) {
                    i += n + close + n;
                } else {
                    i += n;
                }
                continue;
            }
            b'[' if b.get(i + 1) == Some(&b'[') => {
                if let Some(close) = line[i + 2..].find("]]") {
                    let inner_start = i + 2;
                    let inner = &line[inner_start..inner_start + close];
                    if !inner.contains('[') {
                        let embed = i > 0 && b[i - 1] == b'!';
                        let full_start = base + i - usize::from(embed);
                        let full = full_start..base + inner_start + close + 2;
                        push_wikilink(inner, base + inner_start, full, embed, line_no, out);
                        i = inner_start + close + 2;
                        continue;
                    }
                }
            }
            b']' if b.get(i + 1) == Some(&b'(') => {
                if let Some(close) = line[i + 2..].find(')') {
                    let dest_start = i + 2;
                    let dest = &line[dest_start..dest_start + close];
                    let embed = line[..i]
                        .rfind('[')
                        .is_some_and(|open| open > 0 && b[open - 1] == b'!');
                    push_mdlink(dest, base + dest_start, embed, line_no, out);
                    i = dest_start + close + 1;
                    continue;
                }
            }
            b'#' => {
                let at_boundary = i == 0 || b[i - 1].is_ascii_whitespace();
                if at_boundary {
                    let rest = &line[i + 1..];
                    let end = rest
                        .char_indices()
                        .find(|&(_, c)| !is_tag_char(c))
                        .map_or(rest.len(), |(p, _)| p);
                    let tag = rest[..end].trim_end_matches('/');
                    if !tag.is_empty() && !tag.chars().all(|c| c.is_ascii_digit()) {
                        push_unique(&mut out.tags, tag);
                        let start = base + i + 1;
                        out.tag_spans.push(start..start + tag.len());
                    }
                    i += 1 + end;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn push_wikilink(
    inner: &str,
    inner_base: usize,
    full: Range<usize>,
    embed: bool,
    line: usize,
    out: &mut Parsed,
) {
    let (target_part, alias) = match inner.split_once('|') {
        Some((t, a)) => (t, Some(a.trim().to_string())),
        None => (inner, None),
    };
    let (name, heading) = match target_part.split_once('#') {
        Some((n, h)) => (n, Some(h.trim().to_string())),
        None => (target_part, None),
    };
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return; // `[[#見出し]]` は同一ノート内リンク
    }
    let lead = name.len() - name.trim_start().len();
    let start = inner_base + lead;
    out.links.push(Link {
        kind: LinkKind::Wiki,
        target: trimmed.to_string(),
        heading,
        alias,
        embed,
        line,
        span: start..start + trimmed.len(),
        full,
    });
}

fn push_mdlink(dest: &str, dest_base: usize, embed: bool, line: usize, out: &mut Parsed) {
    // `[t](path "title")` のタイトル部分を落とす
    let lead = dest.len() - dest.trim_start().len();
    let mut path = dest.trim();
    if let Some(stripped) = path.strip_prefix('<') {
        // <path with spaces> 形式
        let Some(end) = stripped.find('>') else {
            return;
        };
        path = &stripped[..end];
        return finish_mdlink(path, dest_base + lead + 1, embed, line, out);
    }
    if let Some(sp) = path.find([' ', '\t']) {
        path = &path[..sp];
    }
    finish_mdlink(path, dest_base + lead, embed, line, out)
}

fn finish_mdlink(path: &str, start: usize, embed: bool, line: usize, out: &mut Parsed) {
    let (file, heading) = match path.split_once('#') {
        Some((f, h)) => (f, Some(percent_decode(h))),
        None => (path, None),
    };
    if file.is_empty() || file.contains("://") || file.starts_with("mailto:") {
        return;
    }
    out.links.push(Link {
        kind: LinkKind::Markdown,
        target: percent_decode(file),
        heading,
        alias: None,
        embed,
        line,
        span: start..start + file.len(),
        full: start..start + file.len(),
    });
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Markdownリンクの宛先として安全な形にエンコードする（空白と括弧のみ）。
pub fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_links_tags() {
        let src = "# Title\n\nSee [[Note A|alias]] and ![[img.png]] #project/alpha\n\n## Sub ##\n[md](dir/b%20c.md#sec) [ext](https://x.y)\n";
        let p = parse(src);
        assert_eq!(p.headings.len(), 2);
        assert_eq!(p.headings[1].text, "Sub");
        assert_eq!(p.headings[1].line, 4);
        assert_eq!(p.links.len(), 3);
        assert_eq!(p.links[0].target, "Note A");
        assert_eq!(p.links[0].alias.as_deref(), Some("alias"));
        assert_eq!(&src[p.links[0].span.clone()], "Note A");
        assert_eq!(&src[p.links[0].full.clone()], "[[Note A|alias]]");
        assert!(p.links[1].embed);
        assert_eq!(&src[p.links[1].full.clone()], "![[img.png]]");
        assert_eq!(p.links[2].kind, LinkKind::Markdown);
        assert_eq!(p.links[2].target, "dir/b c.md");
        assert_eq!(p.links[2].heading.as_deref(), Some("sec"));
        assert_eq!(&src[p.links[2].span.clone()], "dir/b%20c.md");
        assert_eq!(p.tags, vec!["project/alpha"]);
        assert_eq!(&src[p.tag_spans[0].clone()], "project/alpha");
    }

    #[test]
    fn tasks_are_collected() {
        let src = "---\na: 1\n---\n- [ ] one 📅 2026-01-02\n```\n- [ ] in code\n```\n  - [x] two";
        let p = parse(src);
        let got: Vec<_> = p
            .tasks
            .iter()
            .map(|t| (t.line, t.status, t.text.as_str()))
            .collect();
        assert_eq!(got, [(3, ' ', "one"), (7, 'x', "two")]);
        assert_eq!(&src[p.tasks[1].marker.clone()], "x");
        assert!(p.tasks[0].due.is_some() && !p.tasks[0].done() && p.tasks[1].done());
    }

    #[test]
    fn code_is_ignored() {
        let src = "```rust\n[[not a link]] #nottag\n```\n`[[inline]]` [[real]]\n#123 #a1\n";
        let p = parse(src);
        assert_eq!(p.links.len(), 1);
        assert_eq!(p.links[0].target, "real");
        assert_eq!(p.links[0].line, 3);
        assert_eq!(p.tags, vec!["a1"]);
    }

    #[test]
    fn frontmatter() {
        let src = "---\ntags: [a, \"b\"]\naliases:\n  - Foo Bar\n  - baz\ntitle: x\n---\n# H\n#c\n";
        let p = parse(src);
        assert_eq!(p.tags, vec!["a", "b", "c"]);
        assert_eq!(p.aliases, vec!["Foo Bar", "baz"]);
        assert_eq!(p.headings[0].line, 7);
        assert_eq!(&src[p.body_start..], "# H\n#c\n");
    }

    #[test]
    fn japanese() {
        let p = parse("## 見出し\n[[日本語ノート#節|表示]] #タグ/子\n");
        assert_eq!(p.headings[0].text, "見出し");
        assert_eq!(p.links[0].target, "日本語ノート");
        assert_eq!(p.links[0].heading.as_deref(), Some("節"));
        assert_eq!(p.tags, vec!["タグ/子"]);
    }

    #[test]
    fn percent() {
        assert_eq!(percent_decode("a%20b%"), "a b%");
        assert_eq!(percent_decode("%E3%81%82"), "あ");
    }
}
