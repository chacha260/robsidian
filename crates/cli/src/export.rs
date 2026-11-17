//! `rob export`: vaultを静的なHTMLサイトとして書き出す。

use anyhow::{bail, Context, Result};
use pulldown_cmark::{html, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use robsidian_core::vault::{file_name, parent_dir};
use robsidian_core::{parse, LinkKind, Vault};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const STYLE: &str = r#":root { color-scheme: light dark; --fg: #222; --bg: #fff; --dim: #777; --accent: #1a6fd6; --code: #f2f3f5; }
@media (prefers-color-scheme: dark) { :root { --fg: #ddd; --bg: #1b1c1f; --dim: #888; --accent: #6cb6ff; --code: #26292f; } }
body { margin: 0; background: var(--bg); color: var(--fg); font: 16px/1.7 system-ui, "Hiragino Sans", "Yu Gothic", "Noto Sans CJK JP", sans-serif; }
nav { padding: 12px 24px; border-bottom: 1px solid color-mix(in srgb, var(--dim) 30%, transparent); }
main { max-width: 760px; margin: 0 auto; padding: 24px; }
a { color: var(--accent); text-decoration: none; } a:hover { text-decoration: underline; }
.unresolved { color: var(--dim); text-decoration: underline dotted; }
pre, code { background: var(--code); border-radius: 4px; } pre { padding: 12px; overflow-x: auto; } code { padding: 0 4px; }
blockquote { margin: 0; padding-left: 12px; border-left: 3px solid var(--dim); color: var(--dim); }
table { border-collapse: collapse; } td, th { border: 1px solid var(--dim); padding: 4px 8px; }
img { max-width: 100%; }
.backlinks { margin-top: 48px; padding-top: 12px; border-top: 1px solid color-mix(in srgb, var(--dim) 30%, transparent); font-size: 14px; }
.tags a { margin-right: 8px; }
"#;

/// 見出しのアンカーにする文字列（英数字と日本語などはそのまま、空白は `-`）。
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        } else if c.is_whitespace() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// URLのパス部分として安全な形にする。
fn encode(path: &str) -> String {
    let mut out = String::new();
    for c in path.chars() {
        match c {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '%' => out.push_str("%25"),
            '"' => out.push_str("%22"),
            c => out.push(c),
        }
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `from` のページから `to`（vault相対のファイル）への相対URL。
fn relative(from: &str, to: &str) -> String {
    let depth = parent_dir(from)
        .split('/')
        .filter(|s| !s.is_empty())
        .count();
    format!("{}{}", "../".repeat(depth), encode(to))
}

fn page_of(note: &str) -> String {
    format!("{}.html", note.strip_suffix(".md").unwrap_or(note))
}

/// ノートの本文のリンクを、書き出し先のHTMLへのリンクに書き換えたMarkdownにする。
fn rewrite_links(vault: &Vault, path: &str, text: &str, attachments: &mut Vec<String>) -> String {
    let parsed = parse::parse(text);
    let mut out = text.to_string();
    for link in parsed.links.iter().rev() {
        let note = vault.resolve(&link.target, path);
        let file = vault.resolve_file(&link.target, path);
        let anchor = link
            .heading
            .as_deref()
            .map(|h| format!("#{}", slug(h)))
            .unwrap_or_default();
        let label = link
            .alias
            .clone()
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| match &link.heading {
                Some(h) => format!("{} › {h}", link.target),
                None => link.target.clone(),
            });
        match link.kind {
            LinkKind::Wiki => {
                let rep = match (note, file) {
                    (_, Some(f)) if link.embed => {
                        attachments.push(f.to_string());
                        format!("![{}]({})", html_escape(&link.target), relative(path, f))
                    }
                    (Some(n), _) => format!(
                        "[{}]({}{anchor})",
                        label.replace(']', "\\]"),
                        relative(path, &page_of(n))
                    ),
                    (None, Some(f)) => {
                        attachments.push(f.to_string());
                        format!("[{}]({})", label.replace(']', "\\]"), relative(path, f))
                    }
                    (None, None) => {
                        format!("<span class=\"unresolved\">{}</span>", html_escape(&label))
                    }
                };
                out.replace_range(link.full.clone(), &rep);
            }
            LinkKind::Markdown => {
                let rep = match (note, file) {
                    (Some(n), _) => format!("{}{anchor}", relative(path, &page_of(n))),
                    (None, Some(f)) => {
                        attachments.push(f.to_string());
                        relative(path, f)
                    }
                    (None, None) => continue,
                };
                out.replace_range(link.span.clone(), &rep);
            }
        }
    }
    out.split_off(parsed.body_start)
}

/// Markdownを、見出しにアンカー（id）を付けてHTMLにする。
fn render(md: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    let mut events: Vec<Event> = Vec::new();
    let mut heading: Option<(HeadingLevel, Vec<Event>)> = None;
    for e in Parser::new_ext(md, options) {
        match e {
            Event::Start(Tag::Heading { level, .. }) => heading = Some((level, Vec::new())),
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, inner)) = heading.take() {
                    let text: String = inner
                        .iter()
                        .filter_map(|e| match e {
                            Event::Text(t) | Event::Code(t) => Some(t.as_ref()),
                            _ => None,
                        })
                        .collect();
                    let mut body = String::new();
                    html::push_html(&mut body, inner.into_iter());
                    let n = level as usize;
                    events.push(Event::Html(
                        format!("<h{n} id=\"{}\">{body}</h{n}>\n", html_escape(&slug(&text)))
                            .into(),
                    ));
                }
            }
            e => match heading.as_mut() {
                Some((_, inner)) => inner.push(e),
                None => events.push(e),
            },
        }
    }
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

fn page(title: &str, root: &str, site: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"ja\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>{t}</title><link rel=\"stylesheet\" href=\"{root}style.css\"></head>\n<body><nav><a href=\"{root}index.html\">{s}</a></nav><main>\n{body}</main></body></html>\n",
        t = html_escape(title),
        s = html_escape(site),
    )
}

/// 書き出したページ数を返す。
pub fn run(vault: &Vault, out: &Path, force: bool) -> Result<usize> {
    if out.exists() && fs::read_dir(out)?.next().is_some() && !force {
        bail!(
            "{} は空ではありません（上書きするなら --force）",
            out.display()
        );
    }
    if out.starts_with(vault.root()) {
        bail!("vaultの中には書き出せません: {}", out.display());
    }
    fs::create_dir_all(out)?;
    fs::write(out.join("style.css"), STYLE)?;
    let site = vault
        .root()
        .file_name()
        .map_or_else(|| "vault".to_string(), |n| n.to_string_lossy().into_owned());
    let mut attachments: Vec<String> = Vec::new();
    let mut count = 0;
    for note in vault.notes() {
        let root = "../".repeat(
            parent_dir(&note.path)
                .split('/')
                .filter(|s| !s.is_empty())
                .count(),
        );
        let md = rewrite_links(vault, &note.path, &note.text, &mut attachments);
        let mut body = render(&md);
        if !note.parsed.tags.is_empty() {
            let tags: Vec<String> = note
                .parsed
                .tags
                .iter()
                .map(|t| format!("#{}", html_escape(t)))
                .collect();
            let _ = writeln!(body, "<p class=\"tags\">{}</p>", tags.join(" "));
        }
        let backlinks = vault.backlinks(&note.path);
        if !backlinks.is_empty() {
            body.push_str("<section class=\"backlinks\"><h2>バックリンク</h2><ul>\n");
            let mut seen = Vec::new();
            for b in backlinks {
                if seen.contains(&&*b.source) {
                    continue;
                }
                seen.push(&*b.source);
                let name = file_name(&b.source).trim_end_matches(".md");
                let _ = writeln!(
                    body,
                    "<li><a href=\"{}\">{}</a></li>",
                    relative(&note.path, &page_of(&b.source)),
                    html_escape(name)
                );
            }
            body.push_str("</ul></section>\n");
        }
        let dest = out.join(page_of(&note.path));
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&dest, page(&note.name, &root, &site, &body))
            .with_context(|| format!("failed to write {}", dest.display()))?;
        count += 1;
    }
    attachments.sort();
    attachments.dedup();
    for f in attachments {
        let dest = out.join(&f);
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::copy(vault.abs(&f), &dest).with_context(|| format!("failed to copy {f}"))?;
    }
    // 目次: フォルダごとにノートを並べる
    let mut folders: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for note in vault.notes() {
        folders
            .entry(parent_dir(&note.path))
            .or_default()
            .push(&note.path);
    }
    let mut index = format!("<h1>{}</h1>\n", html_escape(&site));
    for (folder, notes) in folders {
        if !folder.is_empty() {
            let _ = writeln!(index, "<h2>{}</h2>", html_escape(folder));
        }
        index.push_str("<ul>\n");
        for n in notes {
            let name = file_name(n).trim_end_matches(".md");
            let _ = writeln!(
                index,
                "<li><a href=\"{}\">{}</a></li>",
                encode(&page_of(n)),
                html_escape(name)
            );
        }
        index.push_str("</ul>\n");
    }
    fs::write(out.join("index.html"), page(&site, "", &site, &index))?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_a_site() {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            ("Home.md", "---\ntags: [x]\n---\n# Home\nSee [[notes/Deep Note#Part 2|deep]] and [[missing]] ![[img/pic.png]]"),
            ("notes/Deep Note.md", "## Part 2\nback to [home](../Home.md) #tag"),
            ("img/pic.png", "png"),
        ];
        for (p, t) in files {
            let abs = dir.path().join(p);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(abs, t).unwrap();
        }
        let v = Vault::open(dir.path()).unwrap();
        let out = tempfile::tempdir().unwrap();
        assert_eq!(run(&v, out.path(), false).unwrap(), 2);
        let home = std::fs::read_to_string(out.path().join("Home.html")).unwrap();
        assert!(home.contains("<h1 id=\"home\">Home</h1>"));
        assert!(home.contains("<a href=\"notes/Deep%20Note.html#part-2\">deep</a>"));
        assert!(home.contains("<span class=\"unresolved\">missing</span>"));
        assert!(home.contains("<img src=\"img/pic.png\""));
        assert!(!home.contains("tags: [x]"));
        let deep = std::fs::read_to_string(out.path().join("notes/Deep Note.html")).unwrap();
        assert!(deep.contains("<h2 id=\"part-2\">Part 2</h2>"));
        assert!(deep.contains("href=\"../Home.html\""));
        assert!(deep.contains("<section class=\"backlinks\">") || !deep.contains("バックリンク"));
        assert!(out.path().join("img/pic.png").exists());
        assert!(out.path().join("index.html").exists());
        // 空でない書き出し先は --force が要る
        assert!(run(&v, out.path(), false).is_err());
        assert!(run(&v, out.path(), true).is_ok());
        assert_eq!(slug("Part 2: 日本語 見出し!"), "part-2-日本語-見出し");
    }
}
