//! git連携: 変更件数、スナップショットのコミット、ノートごとのコミット履歴。
//!
//! `git2` を組み込むとビルドが重くなるので、`git` コマンドを呼び出して実装する。
//! vaultがリポジトリの一部（サブフォルダ）でも動くよう、対象はvaultのフォルダ以下に限る。

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::{Command, Output};

/// コミットや変更件数の対象から外すもの（Robsidianがvault内に書く、作業用のファイル）
const EXCLUDES: &[&str] = &[
    ":(exclude).robsidian/history",
    ":(exclude).robsidian/workspace.json",
    ":(exclude).robsidian/cache",
];

fn git(root: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(root)
        // 日本語のファイル名を \343\201... のようにエスケープさせない
        .args(["-c", "core.quotepath=off"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    c
}

fn run(cmd: &mut Command) -> Result<Output> {
    let out = cmd
        .output()
        .context("git を実行できません（インストールされていますか？）")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("git が失敗しました");
        bail!("{}", line.trim());
    }
    Ok(out)
}

/// vaultのフォルダがgitの作業ツリーの中にあるか。
pub fn is_repo(root: &Path) -> bool {
    git(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|o| o.status.success() && o.stdout.starts_with(b"true"))
}

/// vault以下でコミットされていない変更（未追跡を含む）のパス。
pub fn changes(root: &Path) -> Result<Vec<String>> {
    let out = run(git(root)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ])
        .args(EXCLUDES))?;
    let text = String::from_utf8_lossy(&out.stdout);
    // パスはリポジトリのルートからなので、vaultのフォルダからに直す
    let prefix = run(git(root).args(["rev-parse", "--show-prefix"]))?;
    let prefix = String::from_utf8_lossy(&prefix.stdout).trim().to_string();
    let mut paths = Vec::new();
    let mut fields = text.split('\0');
    while let Some(entry) = fields.next() {
        if entry.len() < 4 {
            continue;
        }
        // 改名は「新しい名前\0元の名前」の2つで1件
        if entry.starts_with('R') || entry.starts_with('C') {
            fields.next();
        }
        let path = &entry[3..];
        paths.push(
            path.strip_prefix(prefix.as_str())
                .unwrap_or(path)
                .to_string(),
        );
    }
    Ok(paths)
}

/// vault以下の変更をすべてコミットする。変更が無ければ `None`、あれば短いコミットID。
pub fn snapshot(root: &Path, message: &str) -> Result<Option<String>> {
    run(git(root).args(["add", "-A", "--", "."]).args(EXCLUDES))?;
    // vault以下でステージされたものがあるか
    let staged = git(root)
        .args(["diff", "--cached", "--quiet", "--", "."])
        .status()
        .context("git を実行できません")?;
    if staged.success() {
        return Ok(None);
    }
    // vaultの外でステージされていたものまで巻き込まないよう、vault以下だけをコミットする
    run(git(root).args(["commit", "-q", "-m", message, "--", "."]))?;
    let out = run(git(root).args(["rev-parse", "--short", "HEAD"]))?;
    Ok(Some(
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    /// コミット時刻（UNIX秒）
    pub time: i64,
    pub subject: String,
    /// そのコミットでのファイルの場所（リポジトリのルートから。改名を追いかける）
    pub path: String,
}

impl Commit {
    /// ローカル時刻の `YYYY-MM-DD HH:MM`
    pub fn label(&self) -> String {
        chrono::DateTime::from_timestamp(self.time, 0)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default()
    }
}

/// ノートを変更したコミット（新しい順、最大 `limit` 件）。改名も追いかける。
pub fn log(root: &Path, rel: &str, limit: usize) -> Result<Vec<Commit>> {
    let out = run(git(root).args([
        "log",
        "--follow",
        &format!("-n{limit}"),
        "--name-only",
        "--format=%x1e%H%x1f%ct%x1f%s",
        "--",
        rel,
    ]))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut commits = Vec::new();
    for record in text.split('\x1e').skip(1) {
        let mut lines = record.lines();
        let Some(head) = lines.next() else { continue };
        let mut f = head.split('\x1f');
        let (Some(hash), Some(time), Some(subject)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let Some(path) = lines.find(|l| !l.trim().is_empty()) else {
            continue;
        };
        commits.push(Commit {
            hash: hash.to_string(),
            time: time.parse().unwrap_or(0),
            subject: subject.to_string(),
            path: path.to_string(),
        });
    }
    Ok(commits)
}

/// そのコミットでのノートの内容。消されたコミットなら空文字列。
pub fn show(root: &Path, commit: &Commit) -> Result<String> {
    let spec = format!("{}:{}", commit.hash, commit.path);
    match run(git(root).args(["show", &spec])) {
        Ok(out) => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        // 削除したコミットではファイルが無い
        Err(_) => Ok(String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        sh(dir.path(), &["init", "-q"]);
        sh(dir.path(), &["config", "user.name", "t"]);
        sh(dir.path(), &["config", "user.email", "t@example.com"]);
        sh(dir.path(), &["config", "commit.gpgsign", "false"]);
        dir
    }

    #[test]
    fn snapshot_and_history() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = repo();
        // vaultはリポジトリのサブフォルダ
        let root = dir.path().join("vault");
        fs::create_dir_all(root.join(".robsidian/history")).unwrap();
        fs::write(dir.path().join("outside.txt"), "x").unwrap();
        assert!(is_repo(&root));
        assert!(!is_repo(tempfile::tempdir().unwrap().path()));

        // 改名の検出は内容の類似度で行われるので、変更は一部にとどめる
        let body = "共通の本文\n".repeat(20);
        fs::write(root.join("日本語.md"), format!("one\n{body}")).unwrap();
        fs::write(root.join(".robsidian/history/x"), "h").unwrap();
        fs::write(root.join(".robsidian/workspace.json"), "{}").unwrap();
        assert_eq!(changes(&root).unwrap(), vec!["日本語.md".to_string()]);

        let first = snapshot(&root, "first").unwrap();
        assert!(first.is_some());
        assert!(changes(&root).unwrap().is_empty());
        assert_eq!(snapshot(&root, "nothing").unwrap(), None);

        fs::write(root.join("日本語.md"), format!("two\n{body}")).unwrap();
        snapshot(&root, "second").unwrap().unwrap();
        // 改名しても以前の版を追える
        sh(&root, &["mv", "日本語.md", "renamed.md"]);
        fs::write(root.join("renamed.md"), format!("three\n{body}")).unwrap();
        snapshot(&root, "third").unwrap().unwrap();

        let log = log(&root, "renamed.md", 10).unwrap();
        let subjects: Vec<&str> = log.iter().map(|c| c.subject.as_str()).collect();
        assert_eq!(subjects, ["third", "second", "first"]);
        assert_eq!(log[2].path, "vault/日本語.md");
        assert_eq!(show(&root, &log[2]).unwrap(), format!("one\n{body}"));
        assert_eq!(show(&root, &log[0]).unwrap(), format!("three\n{body}"));
        // vaultの外のファイルと作業用ファイルはコミットされない
        let out = git(dir.path())
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        let status = String::from_utf8_lossy(&out.stdout);
        assert!(status.contains("outside.txt"), "{status}");
        let out = git(dir.path()).args(["ls-files"]).output().unwrap();
        let files = String::from_utf8_lossy(&out.stdout);
        assert!(!files.contains("history"), "{files}");
    }
}
