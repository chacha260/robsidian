//! vaultごとの作業状態（ブックマーク・最近開いたノート）。`.robsidian/workspace.json` に保存する。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 30;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub bookmarks: Vec<String>,
    pub recent: Vec<String>,
}

fn file(root: &Path) -> PathBuf {
    root.join(".robsidian").join("workspace.json")
}

impl Workspace {
    pub fn load(root: &Path) -> Self {
        std::fs::read_to_string(file(root))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) {
        let path = file(root);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn is_bookmarked(&self, path: &str) -> bool {
        self.bookmarks.iter().any(|b| b == path)
    }

    /// 追加したら true、外したら false。
    pub fn toggle_bookmark(&mut self, path: &str) -> bool {
        if self.is_bookmarked(path) {
            self.bookmarks.retain(|b| b != path);
            false
        } else {
            self.bookmarks.push(path.to_string());
            true
        }
    }

    pub fn touch(&mut self, path: &str) {
        self.recent.retain(|r| r != path);
        self.recent.insert(0, path.to_string());
        self.recent.truncate(MAX_RECENT);
    }

    /// ノート（またはフォルダ）の移動に合わせてパスを書き換える。
    pub fn renamed(&mut self, old: &str, new: &str) {
        for path in self.bookmarks.iter_mut().chain(self.recent.iter_mut()) {
            if let Some(moved) = moved_path(path, old, new) {
                *path = moved;
            }
        }
    }

    /// 削除されたノート（またはフォルダ配下）を取り除く。
    pub fn removed(&mut self, gone: &str) {
        let inside = |p: &String| p == gone || p.starts_with(&format!("{gone}/"));
        self.bookmarks.retain(|p| !inside(p));
        self.recent.retain(|p| !inside(p));
    }
}

/// `old` を `new` に移したとき、`path` の移動先。`old` と無関係なら None。
pub fn moved_path(path: &str, old: &str, new: &str) -> Option<String> {
    if path == old {
        return Some(new.to_string());
    }
    path.strip_prefix(old)
        .filter(|rest| rest.starts_with('/'))
        .map(|rest| format!("{new}{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_recent_and_renames() {
        let dir = tempfile::tempdir().unwrap();
        let mut w = Workspace::default();
        assert!(w.toggle_bookmark("a.md"));
        assert!(w.toggle_bookmark("dir/b.md"));
        assert!(!w.toggle_bookmark("a.md"));
        for i in 0..40 {
            w.touch(&format!("n{i}.md"));
        }
        w.touch("dir/b.md");
        assert_eq!(w.recent.len(), MAX_RECENT);
        assert_eq!(w.recent[0], "dir/b.md");

        w.renamed("dir", "moved");
        assert_eq!(w.bookmarks, ["moved/b.md"]);
        assert_eq!(w.recent[0], "moved/b.md");
        w.removed("moved");
        assert!(w.bookmarks.is_empty());

        w.save(dir.path());
        assert_eq!(Workspace::load(dir.path()), w);
        assert_eq!(moved_path("dirx/a.md", "dir", "z"), None);
    }
}
