//! ノート名のあいまい検索と、本文の全文検索。
//!
//! 本文はすべてメモリ上にあるので、全文検索はディスクI/Oなしで全コア並列に走る。

use crate::vault::Vault;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct LineHit {
    /// 0始まりの行番号
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NoteHit {
    pub path: String,
    pub score: u32,
    pub lines: Vec<LineHit>,
}

/// ノートのパスをあいまい検索する（クイックスイッチャー用）。
/// クエリが空のときは更新が新しい順に返す。
pub fn fuzzy(vault: &Vault, query: &str, limit: usize) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() {
        let mut notes: Vec<_> = vault.notes().collect();
        notes.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path)));
        return notes.into_iter().take(limit).map(|n| n.path.clone()).collect();
    }
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, &str)> = Vec::new();
    for note in vault.notes() {
        let hay = note.path.strip_suffix(".md").unwrap_or(&note.path);
        let path_score = pattern.score(Utf32Str::new(hay, &mut buf), &mut matcher);
        // ファイル名そのものに当たる候補を、フォルダ名に当たる候補より上に出す
        let name_score = pattern
            .score(Utf32Str::new(&note.name, &mut buf), &mut matcher)
            .map(|s| s * 2);
        let alias_score = note
            .parsed
            .aliases
            .iter()
            .filter_map(|a| pattern.score(Utf32Str::new(a, &mut buf), &mut matcher))
            .max();
        if let Some(score) = [path_score, name_score, alias_score].into_iter().flatten().max() {
            scored.push((score, &note.path));
        }
    }
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.len().cmp(&b.1.len()))
            .then_with(|| a.1.cmp(b.1))
    });
    scored.into_iter().take(limit).map(|(_, p)| p.to_string()).collect()
}

#[derive(Debug, Default)]
struct Query {
    terms: Vec<Regex>,
    tags: Vec<String>,
    paths: Vec<String>,
}

/// 空白区切りのAND検索。`"フレーズ"`、`tag:名前`、`path:部分文字列` に対応。
fn parse_query(query: &str) -> Query {
    let mut q = Query::default();
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in query.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    for t in tokens {
        if let Some(tag) = t.strip_prefix("tag:") {
            let tag = tag.trim_start_matches('#').to_lowercase();
            if !tag.is_empty() {
                q.tags.push(tag);
            }
        } else if let Some(p) = t.strip_prefix("path:") {
            if !p.is_empty() {
                q.paths.push(p.to_lowercase());
            }
        } else if let Ok(re) = RegexBuilder::new(&regex::escape(&t))
            .case_insensitive(true)
            .build()
        {
            q.terms.push(re);
        }
    }
    q
}

/// 全文検索。すべての条件を満たすノートを、関連度の高い順に返す。
pub fn search(vault: &Vault, query: &str, limit: usize) -> Vec<NoteHit> {
    const MAX_LINES_PER_NOTE: usize = 20;
    let q = parse_query(query);
    if q.terms.is_empty() && q.tags.is_empty() && q.paths.is_empty() {
        return Vec::new();
    }
    let notes: Vec<_> = vault.notes().collect();
    let mut hits: Vec<NoteHit> = notes
        .par_iter()
        .filter_map(|note| {
            let lower_path = note.path.to_lowercase();
            if !q.paths.iter().all(|p| lower_path.contains(p)) {
                return None;
            }
            // `tag:a` は子タグ `a/b` にも一致させる
            let has_tag = |want: &String| {
                note.parsed.tags.iter().any(|t| {
                    let t = t.to_lowercase();
                    t == *want || t.strip_prefix(want.as_str()).is_some_and(|r| r.starts_with('/'))
                })
            };
            if !q.tags.iter().all(has_tag) {
                return None;
            }
            let mut score = 1u32;
            for re in &q.terms {
                let in_name = re.is_match(&note.name);
                let count = re.find_iter(&note.text).take(50).count() as u32;
                if !in_name && count == 0 {
                    return None;
                }
                score += count + if in_name { 100 } else { 0 };
            }
            let mut lines = Vec::new();
            if !q.terms.is_empty() {
                for (i, line) in note.text.lines().enumerate() {
                    if q.terms.iter().any(|re| re.is_match(line)) {
                        lines.push(LineHit {
                            line: i,
                            text: line.trim().to_string(),
                        });
                        if lines.len() >= MAX_LINES_PER_NOTE {
                            break;
                        }
                    }
                }
            }
            Some(NoteHit {
                path: note.path.clone(),
                score,
                lines,
            })
        })
        .collect();
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let files = [
            ("Rust メモ.md", "所有権について\nBorrow checker #lang/rust"),
            ("projects/robsidian.md", "Rust製のノートアプリ\nrust RUST"),
            ("daily/2026-01-01.md", "今日は休み #daily"),
        ];
        for (p, t) in files {
            let abs = dir.path().join(p);
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, t).unwrap();
        }
        let v = Vault::open(dir.path()).unwrap();
        (dir, v)
    }

    #[test]
    fn fuzzy_names() {
        let (_d, v) = vault();
        assert_eq!(fuzzy(&v, "robs", 10), ["projects/robsidian.md"]);
        assert_eq!(fuzzy(&v, "メモ", 10), ["Rust メモ.md"]);
        assert_eq!(fuzzy(&v, "", 10).len(), 3);
        assert!(fuzzy(&v, "zzzz", 10).is_empty());
    }

    #[test]
    fn full_text() {
        let (_d, v) = vault();
        let hits = search(&v, "rust", 10);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "Rust メモ.md"); // ファイル名一致が上
        assert_eq!(hits[1].lines.len(), 2);
        assert_eq!(search(&v, "rust ノート", 10).len(), 1);
        assert_eq!(search(&v, "\"borrow checker\"", 10).len(), 1);
        assert_eq!(search(&v, "tag:lang", 10)[0].path, "Rust メモ.md");
        assert_eq!(search(&v, "path:daily", 10).len(), 1);
        assert!(search(&v, "  ", 10).is_empty());
    }
}
