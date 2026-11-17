//! リンク先を開かずに覗くホバープレビュー。

use crate::preview;
use eframe::egui::{self, RichText};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use robsidian_core::{parse, vault::file_name, Vault};

/// プレビューに出す最大の行数
const MAX_LINES: usize = 40;
pub const WIDTH: f32 = 460.0;

/// ノート本文の抜粋。見出しの指定があればその節（次の同じ深さ以上の見出しまで）、
/// 無ければfrontmatterを除いた冒頭。
pub fn excerpt(text: &str, heading: Option<&str>) -> String {
    let parsed = parse::parse(text);
    let body = &text[parsed.body_start..];
    let lines: Vec<&str> = text.lines().collect();
    let section = heading.and_then(|h| {
        let want = h.rsplit('#').next().unwrap_or(h).trim().to_lowercase();
        let i = parsed
            .headings
            .iter()
            .position(|x| x.text.trim().to_lowercase() == want)?;
        let start = parsed.headings[i].line;
        let level = parsed.headings[i].level;
        let end = parsed.headings[i + 1..]
            .iter()
            .find(|x| x.level <= level)
            .map_or(lines.len(), |x| x.line);
        Some(lines[start..end].join("\n"))
    });
    let src = section.unwrap_or_else(|| body.to_string());
    let mut out: Vec<&str> = src.lines().take(MAX_LINES).collect();
    // コードブロックの途中で切れたら閉じておく（以降が全部コード扱いにならないように）
    let fences = out
        .iter()
        .filter(|l| l.trim_start().starts_with("```") || l.trim_start().starts_with("~~~"))
        .count();
    let truncated = src.lines().count() > MAX_LINES;
    if fences % 2 == 1 {
        out.push("```");
    }
    let mut s = out.join("\n");
    if truncated {
        s.push_str("\n\n…");
    }
    s
}

/// ノートのプレビューを描く。
pub fn note_preview(
    ui: &mut egui::Ui,
    vault: &Vault,
    cache: &mut CommonMarkCache,
    path: &str,
    heading: Option<&str>,
) {
    ui.set_max_width(WIDTH);
    let Some(note) = vault.note(path) else {
        ui.weak(format!("見つかりません: {path}"));
        return;
    };
    let title = file_name(path).trim_end_matches(".md");
    match heading {
        Some(h) => ui.strong(format!("{title} › {h}")),
        None => ui.strong(title),
    };
    ui.weak(RichText::new(path).small());
    ui.separator();
    let src = excerpt(&note.text, heading);
    if src.trim().is_empty() {
        ui.weak("（空のノート）");
        return;
    }
    let src = preview::to_preview(&src, path, vault);
    CommonMarkViewer::new()
        .alerts(preview::alerts())
        .max_image_width(Some(WIDTH as usize - 20))
        .show_with_id(egui::Id::unique(("hover-preview", path)), ui, cache, &src);
}

/// エディタの中に埋め込んだノートを描く（タイトルはリンク）。
pub fn embed_ui(
    ui: &mut egui::Ui,
    vault: &Vault,
    cache: &mut CommonMarkCache,
    path: &str,
    heading: Option<&str>,
) {
    let Some(note) = vault.note(path) else {
        ui.weak(format!("見つかりません: {path}"));
        return;
    };
    let title = match heading {
        Some(h) => format!("{} › {h}", note.name),
        None => note.name.clone(),
    };
    let target = match heading {
        Some(h) => format!("{}#{h}", path.trim_end_matches(".md")),
        None => path.trim_end_matches(".md").to_string(),
    };
    let src = excerpt(&note.text, heading);
    let body = preview::to_preview(&src, path, vault);
    let md = format!(
        "**[{title}]({}{})**\n\n{body}",
        preview::SCHEME,
        preview::encode_target(&target)
    );
    CommonMarkViewer::new()
        .alerts(preview::alerts())
        .max_image_width(Some(ui.available_width().max(80.0) as usize))
        .show_with_id(egui::Id::unique(("embed", path, heading)), ui, cache, &md);
}

/// `ノート#見出し` を (ノート, 見出し) に分ける。
pub fn split_target(target: &str) -> (&str, Option<&str>) {
    match target.split_once('#') {
        Some((t, h)) => (t.trim(), Some(h.trim()).filter(|h| !h.is_empty())),
        None => (target.trim(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpts() {
        let text = "---\na: 1\n---\n# T\nintro\n## A\na1\n### A.1\na2\n## B\nb1";
        assert_eq!(
            excerpt(text, None),
            "# T\nintro\n## A\na1\n### A.1\na2\n## B\nb1"
        );
        assert_eq!(excerpt(text, Some("a")), "## A\na1\n### A.1\na2");
        assert_eq!(excerpt(text, Some("T#B")), "## B\nb1");
        // 見つからない見出しは冒頭を出す
        assert!(excerpt(text, Some("zzz")).starts_with("# T"));
        let long = (0..60)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let e = excerpt(&format!("```\n{long}"), None);
        assert!(e.ends_with("```\n\n…"));
        assert_eq!(split_target("n#h"), ("n", Some("h")));
        assert_eq!(split_target("n#"), ("n", None));
    }
}
