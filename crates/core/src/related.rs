//! 関連ノート: 直接はリンクしていないが、関係の深いノートを見つける。
//!
//! ノートの特徴を「つながっているノート（リンク先とリンク元）」と「タグ」の集合とみなし、
//! 2つのノートの集合の重なり（Jaccard係数）で順位を付ける。
//! - 同じノートからリンクされている（共引用）、同じノートにリンクしている（書誌結合）
//! - 同じタグが付いている
//!
//! のどちらも重なりとして数える。既存の索引だけで計算でき、本文の意味までは見ない。

use crate::Vault;
use serde::Serialize;
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Related {
    pub path: String,
    /// 0〜1。大きいほど関係が深い
    pub score: f32,
    /// どちらのノートともつながっているノート
    pub links: Vec<String>,
    /// 共通のタグ
    pub tags: Vec<String>,
}

/// つながっているノート（リンク先とリンク元。自分自身は除く）
fn neighbors<'a>(vault: &'a Vault, rel: &str) -> BTreeSet<&'a str> {
    let mut set: BTreeSet<&str> = vault
        .outgoing(rel)
        .into_iter()
        .filter_map(|(_, dst)| dst)
        .filter(|dst| vault.note(dst).is_some())
        .collect();
    set.extend(vault.backlinks(rel).iter().map(|b| &*b.source));
    // backlinks の送り元は索引が持つ文字列なので、ノートのパスに揃える
    let mut out = BTreeSet::new();
    for p in set {
        if p != rel {
            if let Some(n) = vault.note(p) {
                out.insert(n.path.as_str());
            }
        }
    }
    out
}

fn tags(vault: &Vault, rel: &str) -> BTreeSet<String> {
    vault.note(rel).map_or_else(BTreeSet::new, |n| {
        n.parsed.tags.iter().map(|t| t.to_lowercase()).collect()
    })
}

/// つながっているノートのおおよその数（リンクの数 + 被リンクの数）。
/// 重複を除くために候補ごとにリンクを解決すると、密なvaultでは候補が数千になり遅いので、件数で近似する。
fn approx_degree(vault: &Vault, rel: &str) -> usize {
    vault.note(rel).map_or(0, |n| n.parsed.links.len()) + vault.backlinks(rel).len()
}

/// タグを小文字に揃える（すでに小文字なら文字列を作らない）
fn lower(tag: &str) -> Cow<'_, str> {
    if tag.chars().any(char::is_uppercase) {
        Cow::Owned(tag.to_lowercase())
    } else {
        Cow::Borrowed(tag)
    }
}

/// `rel` に関係の深いノート（スコアの高い順に最大 `limit` 件）。
/// すでに直接リンクでつながっているノートは除く（それは被リンク欄やリンクで見える）。
pub fn related(vault: &Vault, rel: &str, limit: usize) -> Vec<Related> {
    let Some(me) = vault.note(rel) else {
        return Vec::new();
    };
    let rel = me.path.as_str();
    let my_links = neighbors(vault, rel);
    let my_tags = tags(vault, rel);
    let excluded = |p: &str| p == rel || my_links.contains(p);

    // 候補ごとの (共通のつながり, 共通のタグ)。
    // つながっているノート x の、さらに先のノート y は、x を共有している（共引用・書誌結合）
    let mut shared: HashMap<&str, (Vec<&str>, Vec<String>)> = HashMap::new();
    for x in &my_links {
        for y in neighbors(vault, x) {
            if !excluded(y) {
                shared.entry(y).or_default().0.push(x);
            }
        }
    }
    if !my_tags.is_empty() {
        for note in vault.notes() {
            if excluded(&note.path) {
                continue;
            }
            let common: BTreeSet<String> = note
                .parsed
                .tags
                .iter()
                .map(|t| lower(t))
                .filter(|t| my_tags.contains(t.as_ref()))
                .map(Cow::into_owned)
                .collect();
            if !common.is_empty() {
                shared.entry(note.path.as_str()).or_default().1 = common.into_iter().collect();
            }
        }
    }

    let mut out: Vec<Related> = shared
        .into_iter()
        .map(|(path, (links, common_tags))| {
            let n = links.len() + common_tags.len();
            let their_tags = vault.note(path).map_or(0, |n| n.parsed.tags.len());
            // 和集合 = 自分 + 相手 - 共通（つながりとタグそれぞれ）
            let union = (my_links.len() + approx_degree(vault, path)).saturating_sub(links.len())
                + (my_tags.len() + their_tags).saturating_sub(common_tags.len());
            Related {
                path: path.to_string(),
                score: n as f32 / union.max(n) as f32,
                links: links.into_iter().map(str::to_string).collect(),
                tags: common_tags,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.path.cmp(&b.path))
    });
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_notes_sharing_links_and_tags() {
        let dir = tempfile::tempdir().unwrap();
        let w = |p: &str, s: &str| fs::write(dir.path().join(p), s).unwrap();
        // a と b はどちらも x と y にリンクしている（直接はつながっていない）
        w("a.md", "[[x]] [[y]] #rust");
        w("b.md", "[[x]] [[y]] #rust");
        // c は x にだけリンクしている
        w("c.md", "[[x]]");
        // d はタグだけが同じ
        w("d.md", "#Rust");
        // e は a に直接リンクしているので候補にならない
        w("e.md", "[[a]] [[x]] #rust");
        w("x.md", "");
        w("y.md", "");
        w("lonely.md", "何もない");
        let vault = Vault::open(dir.path()).unwrap();

        let r = related(&vault, "a.md", 10);
        let paths: Vec<&str> = r.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, ["b.md", "c.md", "d.md"]);
        assert_eq!(r[0].links, ["x.md", "y.md"]);
        assert_eq!(r[0].tags, ["rust"]);
        // a のつながり {x, y, e} とタグ {rust}、b は {x, y} と {rust} → 共通3 / 和集合4
        assert!((r[0].score - 0.75).abs() < 1e-6, "{}", r[0].score);
        // c は x だけ、d はタグだけが共通 → どちらも 1/4（同点はパス順）
        assert!((r[1].score - 0.25).abs() < 1e-6, "{}", r[1].score);
        assert!((r[2].score - 0.25).abs() < 1e-6, "{}", r[2].score);
        assert_eq!(related(&vault, "a.md", 1).len(), 1);
        assert!(related(&vault, "lonely.md", 10).is_empty());
        assert!(related(&vault, "nope.md", 10).is_empty());
    }
}
