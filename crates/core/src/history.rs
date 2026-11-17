//! ノートのローカル履歴。
//!
//! 保存で上書きされる直前の内容を `.robsidian/history/<ノートのパス>/<時刻>.md` に残す。
//! 外部エディタやObsidian本体との競合、うっかりした上書きから復元できるようにするためのもの。
//! `.robsidian` は隠しフォルダなので、vaultの索引には載らない。

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 1ノートあたりに残す版の数
pub const KEEP_VERSIONS: usize = 50;
/// これより古い版は消す（ただし最新の数件は日数に関係なく残す）
pub const KEEP_DAYS: u64 = 30;
const ALWAYS_KEEP: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub file: PathBuf,
    /// 保存された時刻（UNIXミリ秒）
    pub millis: u64,
}

impl Version {
    pub fn read(&self) -> Result<String> {
        fs::read_to_string(&self.file)
            .with_context(|| format!("cannot read {}", self.file.display()))
    }

    /// ローカル時刻の `YYYY-MM-DD HH:MM:SS`
    pub fn label(&self) -> String {
        chrono::DateTime::from_timestamp_millis(self.millis as i64)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default()
    }
}

fn dir_of(root: &Path, rel: &str) -> PathBuf {
    root.join(".robsidian").join("history").join(rel)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// 新しい順の版一覧。
pub fn list(root: &Path, rel: &str) -> Vec<Version> {
    let Ok(entries) = fs::read_dir(dir_of(root, rel)) else {
        return Vec::new();
    };
    let mut versions: Vec<Version> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name();
            let millis = name.to_str()?.strip_suffix(".md")?.parse().ok()?;
            Some(Version {
                file: e.path(),
                millis,
            })
        })
        .collect();
    versions.sort_by_key(|v| std::cmp::Reverse(v.millis));
    versions
}

/// `text` を1つの版として残す。直近の版と同じ内容、または空なら何もしない。
pub fn snapshot(root: &Path, rel: &str, text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    let versions = list(root, rel);
    if versions.first().and_then(|v| v.read().ok()).as_deref() == Some(text) {
        return Ok(());
    }
    let dir = dir_of(root, rel);
    fs::create_dir_all(&dir)?;
    // 同じミリ秒に2回保存されても上書きしないよう、空いている時刻をずらして使う
    let mut millis = now_millis().max(versions.first().map_or(0, |v| v.millis + 1));
    while dir.join(format!("{millis}.md")).exists() {
        millis += 1;
    }
    fs::write(dir.join(format!("{millis}.md")), text)?;
    prune(&list(root, rel), now_millis());
    Ok(())
}

fn prune(versions: &[Version], now: u64) {
    let max_age = Duration::from_secs(KEEP_DAYS * 24 * 3600).as_millis() as u64;
    for (i, v) in versions.iter().enumerate() {
        let too_many = i >= KEEP_VERSIONS;
        let too_old = i >= ALWAYS_KEEP && now.saturating_sub(v.millis) > max_age;
        if too_many || too_old {
            let _ = fs::remove_file(&v.file);
        }
    }
}

/// ノート（またはフォルダ）の移動に合わせて履歴も移す。
pub fn rename(root: &Path, old: &str, new: &str) {
    let (from, to) = (dir_of(root, old), dir_of(root, new));
    if from.is_dir() && !to.exists() {
        if let Some(parent) = to.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(from, to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_list_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        snapshot(root, "a/b.md", "v1").unwrap();
        snapshot(root, "a/b.md", "v1").unwrap(); // 同じ内容は残さない
        snapshot(root, "a/b.md", "").unwrap(); // 空は残さない
        snapshot(root, "a/b.md", "v2").unwrap();
        let versions = list(root, "a/b.md");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].read().unwrap(), "v2");
        assert_eq!(versions[1].read().unwrap(), "v1");
        assert!(!versions[0].label().is_empty());

        for i in 0..KEEP_VERSIONS + 5 {
            snapshot(root, "a/b.md", &format!("n{i}")).unwrap();
        }
        assert_eq!(list(root, "a/b.md").len(), KEEP_VERSIONS);

        rename(root, "a", "c");
        assert!(list(root, "a/b.md").is_empty());
        assert_eq!(list(root, "c/b.md").len(), KEEP_VERSIONS);
    }

    #[test]
    fn old_versions_expire() {
        let dir = tempfile::tempdir().unwrap();
        let day = 24 * 3600 * 1000;
        let versions: Vec<Version> = (0..8)
            .map(|i| {
                let file = dir.path().join(format!("{i}.md"));
                fs::write(&file, "x").unwrap();
                Version {
                    file,
                    millis: 100 * day - i * 10 * day,
                }
            })
            .collect();
        prune(&versions, 100 * day);
        // 最新の5件は残し、6件目以降は30日を過ぎたものだけ消える
        let left: Vec<bool> = versions.iter().map(|v| v.file.exists()).collect();
        assert_eq!(left, [true, true, true, true, true, false, false, false]);
    }
}
