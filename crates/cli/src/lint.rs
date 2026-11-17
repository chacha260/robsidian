//! `rob lint`: vaultの問題を `パス:行: 内容` の形で報告する。

use robsidian_core::{LinkKind, Vault};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    /// 1始まりの行番号（ノート全体の問題は0）
    pub line: usize,
    pub level: Level,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
}

pub fn run(vault: &Vault, orphans: bool) -> Vec<Issue> {
    let mut issues = Vec::new();
    // 同じ名前のノート（名前だけのリンクがどれを指すか曖昧になる）
    let mut by_name: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for note in vault.notes() {
        by_name
            .entry(note.name.to_lowercase())
            .or_default()
            .push(&note.path);
    }
    for note in vault.notes() {
        for link in &note.parsed.links {
            let resolved = vault.resolve(&link.target, &note.path);
            let file = vault.resolve_file(&link.target, &note.path);
            let shown = match link.kind {
                LinkKind::Wiki if link.embed => format!("![[{}]]", link.target),
                LinkKind::Wiki => format!("[[{}]]", link.target),
                LinkKind::Markdown => format!("({})", link.target),
            };
            if resolved.is_none() && file.is_none() {
                let (code, message) = if link.embed {
                    (
                        "broken-embed",
                        format!("埋め込み先が見つかりません: {shown}"),
                    )
                } else {
                    (
                        "unresolved-link",
                        format!("リンク先が見つかりません: {shown}"),
                    )
                };
                issues.push(Issue {
                    path: note.path.clone(),
                    line: link.line + 1,
                    level: Level::Error,
                    code,
                    message,
                });
                continue;
            }
            let name_only = link.kind == LinkKind::Wiki && !link.target.contains('/');
            if let (true, Some(dst)) = (name_only, resolved) {
                let key = link.target.trim().trim_end_matches(".md").to_lowercase();
                if let Some(same) = by_name.get(&key).filter(|v| v.len() > 1) {
                    issues.push(Issue {
                        path: note.path.clone(),
                        line: link.line + 1,
                        level: Level::Warning,
                        code: "ambiguous-link",
                        message: format!(
                            "同名のノートが{}つあります。{shown} は {dst} を指します（パスで書くと確実です）",
                            same.len()
                        ),
                    });
                }
            }
        }
    }
    if orphans {
        for path in vault.orphans() {
            issues.push(Issue {
                path: path.to_string(),
                line: 0,
                level: Level::Warning,
                code: "orphan",
                message: "どのノートともリンクでつながっていません".into(),
            });
        }
    }
    issues.sort_by(|a, b| (&a.path, a.line, a.level).cmp(&(&b.path, b.line, b.level)));
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_problems() {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            ("a.md", "[[b]]\n[[missing]] ![[nope.png]]\n[[dup]]"),
            ("b.md", "[ok](a.md)"),
            ("x/dup.md", ""),
            ("y/dup.md", ""),
            ("lonely.md", "alone"),
        ];
        for (p, t) in files {
            let abs = dir.path().join(p);
            std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
            std::fs::write(abs, t).unwrap();
        }
        let v = Vault::open(dir.path()).unwrap();
        let got: Vec<(String, usize, &str)> = run(&v, true)
            .into_iter()
            .map(|i| (i.path, i.line, i.code))
            .collect();
        assert_eq!(
            got,
            [
                ("a.md".into(), 2, "unresolved-link"),
                ("a.md".into(), 2, "broken-embed"),
                ("a.md".into(), 3, "ambiguous-link"),
                ("lonely.md".into(), 0, "orphan"),
                // [[dup]] は x/dup.md を指すので、y/dup.md はどこからもリンクされていない
                ("y/dup.md".into(), 0, "orphan"),
            ]
        );
    }
}
