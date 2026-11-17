//! プレビュー用にMarkdownを変換する。
//!
//! `[[wikilink]]` をクリック可能な通常のMarkdownリンク（`rob:` スキーム）へ、
//! 埋め込み画像を絶対パスの `file://` URIへ書き換える。

use robsidian_core::search::{self, QueryResult};
use robsidian_core::{parse, LinkKind, Vault};
use std::ops::Range;

pub const SCHEME: &str = "rob:";

const IMAGE_EXTS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"];

pub fn is_image(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| IMAGE_EXTS.contains(&ext.to_lowercase().as_str()))
}

/// URLとして安全な文字以外をすべてパーセントエンコードする。
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `rob:` リンクから元のリンク先文字列を取り出す。
pub fn decode_link(url: &str) -> Option<String> {
    url.strip_prefix(SCHEME).map(parse::percent_decode)
}

pub fn file_uri(vault: &Vault, rel: &str) -> String {
    let abs = vault.abs(rel).display().to_string().replace('\\', "/");
    if abs.starts_with('/') {
        format!("file://{abs}")
    } else {
        format!("file:///{abs}")
    }
}

pub fn to_preview(text: &str, from: &str, vault: &Vault) -> String {
    let today = chrono::Local::now().date_naive();
    let expanded = expand_queries(text, vault, today);
    let text = expanded.as_str();
    let parsed = parse::parse(text);
    let mut out = text.to_string();
    for link in parsed.links.iter().rev() {
        let image = link
            .embed
            .then(|| vault.resolve_file(&link.target, from))
            .flatten()
            .filter(|p| is_image(p));
        match link.kind {
            LinkKind::Wiki => {
                let replacement = if let Some(img) = image {
                    format!("![{}](<{}>)", link.target, file_uri(vault, img))
                } else {
                    let label = match (&link.alias, &link.heading) {
                        (Some(a), _) if !a.is_empty() => a.clone(),
                        (_, Some(h)) => format!("{} › {h}", link.target),
                        _ => link.target.clone(),
                    };
                    let target = match &link.heading {
                        Some(h) => format!("{}#{h}", link.target),
                        None => link.target.clone(),
                    };
                    format!("[{label}]({SCHEME}{})", encode(&target))
                };
                out.replace_range(link.full.clone(), &replacement);
            }
            LinkKind::Markdown => {
                if let Some(img) = image {
                    out.replace_range(link.span.clone(), &format!("<{}>", file_uri(vault, img)));
                } else if !link.embed && vault.resolve(&link.target, from).is_some() {
                    out.replace_range(
                        link.span.clone(),
                        &format!("{SCHEME}{}", encode(&link.target)),
                    );
                }
            }
        }
    }
    out.split_off(parsed.body_start)
}

/// ` ```rob-query ` ブロックを、条件に合うノートやタスクのリストに置き換える。
/// タスクはチェックボックスにしない（本文のチェックボックスとの対応がずれないように）。
pub fn expand_queries(text: &str, vault: &Vault, today: chrono::NaiveDate) -> String {
    if !text.contains("rob-query") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut lines = text.split_inclusive('\n');
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        let fence = t
            .strip_prefix("```")
            .filter(|rest| rest.trim() == "rob-query");
        if fence.is_none() {
            out.push_str(line);
            continue;
        }
        let mut query = String::new();
        let mut closed = false;
        for inner in lines.by_ref() {
            if inner.trim_start().starts_with("```") {
                closed = true;
                break;
            }
            query.push_str(inner.trim());
            query.push(' ');
        }
        if !closed {
            // 閉じていないブロックは書きかけとみなし、そのまま見せる
            out.push_str(line);
            out.push_str(&query);
            continue;
        }
        out.push_str(&render_query(query.trim(), vault, today));
    }
    out
}

fn note_link(path: &str) -> String {
    let stem = path.strip_suffix(".md").unwrap_or(path);
    let name = stem.rsplit('/').next().unwrap_or(stem);
    format!("[[{stem}|{name}]]")
}

fn render_query(query: &str, vault: &Vault, today: chrono::NaiveDate) -> String {
    let mut md = String::new();
    let errors = search::query_errors(query);
    let caption = |n: usize| format!("> クエリ `{query}` ・{n}件\n\n");
    match search::run_query(vault, query, today) {
        QueryResult::Notes(hits) => {
            md.push_str(&caption(hits.len()));
            for h in &hits {
                md.push_str(&format!("- {}\n", note_link(&h.path)));
            }
        }
        QueryResult::Tasks(tasks) => {
            md.push_str(&caption(tasks.len()));
            for t in &tasks {
                let mark = if t.task.done() { "■" } else { "□" };
                let due = match t.task.due {
                    Some(d) if d < today && !t.task.done() => format!(" **（期限切れ {d}）**"),
                    Some(d) => format!("（期日 {d}）"),
                    None => String::new(),
                };
                md.push_str(&format!(
                    "- {mark} {}{due} — {}\n",
                    t.task.text,
                    note_link(&t.path)
                ));
            }
        }
    }
    if !errors.is_empty() {
        md.push_str(&format!("\n*解釈できない条件: {}*\n", errors.join(", ")));
    }
    md.push('\n');
    md
}

/// プレビューの描画と同じ解析で、タスクのチェックボックス `[ ]` の位置を順に返す。
pub fn task_markers(text: &str) -> Vec<Range<usize>> {
    use pulldown_cmark::{Event, Options, Parser};
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST;
    Parser::new_ext(text, options)
        .into_offset_iter()
        .filter_map(|(e, r)| matches!(e, Event::TaskListMarker(_)).then_some(r))
        .collect()
}

/// プレビューでチェックボックスが切り替えられたとき（`before` → `after`）、元のノートの本文で
/// 同じ番号のタスクを切り替えた本文を返す。
pub fn toggle_from_preview(source: &str, before: &str, after: &str) -> Option<String> {
    let at = before
        .bytes()
        .zip(after.bytes())
        .position(|(a, b)| a != b)?;
    let k = task_markers(before)
        .iter()
        .position(|r| r.start <= at && at < r.end)?;
    let body = parse::parse(source).body_start;
    let marker = task_markers(&source[body..]).into_iter().nth(k)?;
    let (start, end) = (body + marker.start, body + marker.end);
    // `[ ]` / `[x]` の中身（1文字）を、プレビュー側の新しい状態に合わせる
    let checked = after[at..].starts_with('x') || after[at..].starts_with('X');
    let inner = &source[start + 1..end - 1];
    let mut out = source.to_string();
    out.replace_range(
        start + 1..start + 1 + inner.len(),
        if checked { "x" } else { " " },
    );
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_checkbox_maps_to_source() {
        let source = "---\ntags: [a]\n---\n- [ ] one [[Link]]\n```\n- [ ] code\n```\n- [x] two\n";
        let before = "- [ ] one [Link](rob:Link)\n```\n- [ ] code\n```\n- [x] two\n";
        let after = before.replacen("[x]", "[ ]", 1);
        let out = toggle_from_preview(source, before, &after).unwrap();
        assert!(out.ends_with("- [ ] two\n"));
        let after = before.replacen("[ ] one", "[x] one", 1);
        let out = toggle_from_preview(source, before, &after).unwrap();
        assert!(out.contains("- [x] one [[Link]]"));
        assert!(out.contains("- [ ] code"));
        assert!(toggle_from_preview(source, before, before).is_none());
    }

    #[test]
    fn query_blocks_expand() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.md"),
            "- [ ] 牛乳 📅 2026-01-01\n- [x] 済",
        )
        .unwrap();
        std::fs::write(dir.path().join("b.md"), "#tag").unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let today = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        let src = "x\n```rob-query\ntask:todo\n```\ny\n```rob-query\ntag:tag\n```\n";
        let out = expand_queries(src, &v, today);
        assert!(out.contains("- □ 牛乳 **（期限切れ 2026-01-01）** — [[a|a]]"));
        assert!(out.contains("- [[b|b]]"));
        assert!(out.starts_with("x\n") && out.contains("y\n"));
        // 結果にチェックボックスは無い
        assert!(task_markers(&out).is_empty());
        let unclosed = "```rob-query\ntask:todo";
        assert!(expand_queries(unclosed, &v, today).contains("rob-query"));
    }
}
