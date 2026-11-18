//! Vault（ノートフォルダ）のインメモリ索引。
//!
//! 起動時に全ノートを並列に読み込み、リンク・バックリンク・タグを索引化する。
//! 本文もメモリに保持するので、検索やバックリンク表示はディスクI/Oなしで返る。
//! Obsidianのvaultとそのまま互換（`[[wikilink]]`、`#tag`、frontmatter）。

use crate::history;
use crate::parse::{self, Link, LinkKind, Parsed};
use crate::template;
use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct Note {
    /// vaultルートからの相対パス（区切りは常に `/`、拡張子付き）
    pub path: String,
    /// 拡張子を除いたファイル名
    pub name: String,
    pub text: String,
    pub parsed: Parsed,
    /// 最終更新（UNIX秒）
    pub mtime: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Backlink {
    /// リンク元のパス（同じノートからのバックリンクで共有する）
    pub source: std::sync::Arc<str>,
    /// リンクが書かれている行（0始まり）。行の内容は [`Vault::line`] で取れる
    pub line: usize,
}

/// リンクになっていない言及（[`Vault::unlinked_mentions`]）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mention {
    pub source: String,
    /// 0始まりの行番号
    pub line: usize,
    /// 本文中のバイト範囲
    pub range: std::ops::Range<usize>,
    /// 本文中に書かれていた文字列（大文字小文字は本文のまま）
    pub text: String,
    /// 前後の空白を除いた行
    pub context: String,
    /// `context` の中で言及が始まるバイト位置
    pub column: usize,
}

/// 言及として数えない範囲（既存のリンク全体）。Markdownリンクは `[文字](先)` 全体にする。
fn mention_guard(text: &str, link: &Link) -> std::ops::Range<usize> {
    match link.kind {
        LinkKind::Wiki => link.full.clone(),
        LinkKind::Markdown => {
            let line_start = text[..link.span.start].rfind('\n').map_or(0, |i| i + 1);
            let open = text[line_start..link.span.start]
                .rfind('[')
                .map_or(link.span.start, |i| line_start + i);
            let close = text[link.span.end..]
                .find(')')
                .map_or(link.span.end, |i| link.span.end + i + 1);
            open..close
        }
    }
}

/// 行内のインラインコード（`...`）の範囲。
fn inline_code_ranges(line: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            let n = b[i..].iter().take_while(|&&c| c == b'`').count();
            let ticks = &line[i..i + n];
            if let Some(close) = line[i + n..].find(ticks) {
                out.push(i..i + n + close + n);
                i += n + close + n;
                continue;
            }
            i += n;
            continue;
        }
        i += 1;
    }
    out
}

/// 差し替える前のノートの状態: (別名, (解決できたリンク先, 解決できなかったリンク先))
type OldNote = (Vec<String>, (Vec<String>, Vec<String>));

#[derive(Debug, Default)]
pub struct Vault {
    root: PathBuf,
    notes: BTreeMap<String, Note>,
    files: BTreeSet<String>,
    dirs: BTreeSet<String>,
    lower_paths: HashMap<String, String>,
    by_name: HashMap<String, Vec<String>>,
    by_alias: HashMap<String, Vec<String>>,
    file_names: HashMap<String, Vec<String>>,
    backlinks: HashMap<String, Vec<Backlink>>,
    unresolved: BTreeMap<String, Vec<String>>,
    /// 解決できるリンクを1本以上持つノート（自己リンクも含む。孤立ノートの判定用）
    linking: HashSet<String>,
    /// 上書き前の内容をローカル履歴に残す量
    pub history_limits: history::Limits,
}

pub fn is_note_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

fn strip_note_ext(path: &str) -> &str {
    let lower = path.to_lowercase();
    if lower.ends_with(".md") {
        &path[..path.len() - 3]
    } else if lower.ends_with(".markdown") {
        &path[..path.len() - 9]
    } else {
        path
    }
}

pub fn parent_dir(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

pub fn file_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |i| &path[i + 1..])
}

/// `dir` を基準に `rel` を解決する（`.` と `..` を畳む）。ルートより上に出たら None。
fn join_normalized(dir: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = if rel.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// `from_dir` から `to` への相対パス。
fn relative_path(from_dir: &str, to: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').collect();
    let common = from
        .iter()
        .zip(&to_parts[..to_parts.len() - 1])
        .take_while(|(a, b)| a == b)
        .count();
    let mut out: Vec<&str> = vec![".."; from.len() - common];
    out.extend(&to_parts[common..]);
    out.join("/")
}

/// Windowsの `canonicalize` が付ける `\\?\` 接頭辞を、通常のドライブパスなら外す
/// （表示が読みにくく、`file://` URIや他ツールへの受け渡しでも扱いづらいため）。
fn simplify_path(path: PathBuf) -> PathBuf {
    if cfg!(windows) {
        if let Some(rest) = path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
            if rest.as_bytes().get(1) == Some(&b':') {
                return PathBuf::from(rest);
            }
        }
    }
    path
}

fn mtime_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn read_note(root: &Path, rel: &str) -> Option<Note> {
    let abs = root.join(rel);
    let bytes = fs::read(&abs).ok()?;
    let text = String::from_utf8(bytes)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    Some(make_note(rel, text, mtime_secs(&abs)))
}

fn make_note(rel: &str, text: String, mtime: u64) -> Note {
    let parsed = parse::parse(&text);
    Note {
        path: rel.to_string(),
        name: strip_note_ext(file_name(rel)).to_string(),
        text,
        parsed,
        mtime,
    }
}

/// 同じディレクトリに一時ファイルを書いてからrenameする（書き込み途中のクラッシュで壊れない）。
fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path.parent().context("no parent directory")?;
    let name = path.file_name().context("no file name")?.to_string_lossy();
    let tmp = dir.join(format!(".{name}.rob-tmp"));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    Ok(())
}

impl Vault {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let root = root
            .canonicalize()
            .map(simplify_path)
            .with_context(|| format!("vault not found: {}", root.display()))?;
        if !root.is_dir() {
            bail!("vault is not a directory: {}", root.display());
        }
        let mut v = Vault {
            root,
            ..Default::default()
        };
        v.rescan();
        Ok(v)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// 絶対パスをvault相対パスに変換する（vault外や隠しフォルダ内は None）。
    pub fn rel(&self, abs: &Path) -> Option<String> {
        let rel = abs.strip_prefix(&self.root).ok()?;
        let mut parts = Vec::new();
        for c in rel.components() {
            let Component::Normal(s) = c else { return None };
            let s = s.to_str()?;
            if s.starts_with('.') {
                return None;
            }
            parts.push(s);
        }
        (!parts.is_empty()).then(|| parts.join("/"))
    }

    /// ディスクを走査して索引を作り直す。
    pub fn rescan(&mut self) {
        let mut note_paths = Vec::new();
        self.files.clear();
        self.dirs.clear();
        let walker = WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
        for entry in walker.filter_map(|e| e.ok()) {
            if entry.depth() == 0 {
                continue;
            }
            let Some(rel) = self.rel(entry.path()) else {
                continue;
            };
            if entry.file_type().is_dir() {
                self.dirs.insert(rel);
            } else if is_note_path(&rel) {
                note_paths.push(rel);
            } else {
                self.files.insert(rel);
            }
        }
        let root = &self.root;
        self.notes = note_paths
            .par_iter()
            .filter_map(|rel| read_note(root, rel).map(|n| (rel.clone(), n)))
            .collect();
        self.rebuild_links();
    }

    /// 1ファイルだけ読み直す（外部エディタでの変更検知用）。索引が変わったら true。
    pub fn reload(&mut self, rel: &str) -> bool {
        if !is_note_path(rel) {
            return false;
        }
        let old = self
            .notes
            .get(rel)
            .map(|n| (n.parsed.aliases.clone(), self.old_targets(rel)));
        let changed = match read_note(&self.root, rel) {
            Some(note) => {
                let same = self.notes.get(rel).is_some_and(|old| old.text == note.text);
                self.notes.insert(rel.to_string(), note);
                !same
            }
            None => self.notes.remove(rel).is_some(),
        };
        if changed {
            self.reindex_note(rel, old);
        }
        changed
    }

    /// 名前の索引（パス・ファイル名・別名）と、リンクの索引を作り直す。
    fn rebuild_links(&mut self) {
        self.rebuild_names();
        let notes: Vec<&Note> = self.notes.values().collect();
        // リンク解決はノートごとに独立なので並列に行い、最後にパス順で集約する
        #[allow(clippy::type_complexity)]
        let per_note: Vec<(&str, Vec<(&str, Backlink)>, Vec<&str>, bool)> = notes
            .par_iter()
            .filter(|note| !note.parsed.links.is_empty())
            .map(|note| {
                let (found, missing, any) = self.note_links(note);
                (note.path.as_str(), found, missing, any)
            })
            .collect();
        let mut backlinks: HashMap<String, Vec<Backlink>> = HashMap::new();
        let mut unresolved: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut linking = HashSet::new();
        for (source, found, missing, any) in per_note {
            for (dst, b) in found {
                match backlinks.get_mut(dst) {
                    Some(list) => list.push(b),
                    None => {
                        backlinks.insert(dst.to_string(), vec![b]);
                    }
                }
            }
            for target in &missing {
                unresolved
                    .entry(target.to_string())
                    .or_default()
                    .push(source.to_string());
            }
            if any {
                linking.insert(source.to_string());
            }
        }
        self.backlinks = backlinks;
        self.unresolved = unresolved;
        self.linking = linking;
    }

    fn rebuild_names(&mut self) {
        self.lower_paths.clear();
        self.by_name.clear();
        self.by_alias.clear();
        self.file_names.clear();
        for note in self.notes.values() {
            self.lower_paths
                .insert(strip_note_ext(&note.path).to_lowercase(), note.path.clone());
            self.by_name
                .entry(note.name.to_lowercase())
                .or_default()
                .push(note.path.clone());
            for alias in &note.parsed.aliases {
                self.by_alias
                    .entry(alias.to_lowercase())
                    .or_default()
                    .push(note.path.clone());
            }
        }
        for f in &self.files {
            self.file_names
                .entry(file_name(f).to_lowercase())
                .or_default()
                .push(f.clone());
        }
    }

    /// ノートのリンクを解決する: (解決できたリンク先とバックリンク, 解決できなかったリンク先,
    /// 解決できるリンクを1本でも持つか（自己リンクも含む。孤立ノートの判定用）)。
    #[allow(clippy::type_complexity)]
    fn note_links<'a>(&'a self, note: &'a Note) -> (Vec<(&'a str, Backlink)>, Vec<&'a str>, bool) {
        let source: std::sync::Arc<str> = note.path.as_str().into();
        let mut found: Vec<(&str, Backlink)> = Vec::new();
        let mut missing: Vec<&str> = Vec::new();
        let mut any = false;
        for link in &note.parsed.links {
            if let Some(dst) = self.resolve(&link.target, &note.path) {
                any = true;
                // 自己リンクと、同じ行からの重複リンクは数えない
                if dst == note.path || found.iter().any(|(d, b)| *d == dst && b.line == link.line) {
                    continue;
                }
                found.push((
                    dst,
                    Backlink {
                        source: source.clone(),
                        line: link.line,
                    },
                ));
            } else if self.resolve_file(&link.target, &note.path).is_none()
                && !missing.contains(&link.target.as_str())
            {
                missing.push(&link.target);
            }
        }
        (found, missing, any)
    }

    /// 1つのノートのリンクだけを索引に反映し直す（名前の索引は変わっていない前提）。
    /// `old` は差し替える前のノートのリンク先（解決できたもの, できなかったもの）。
    fn relink(&mut self, rel: &str, old: (Vec<String>, Vec<String>)) {
        let (dsts, missing) = old;
        for d in dsts {
            if let Some(list) = self.backlinks.get_mut(&d) {
                list.retain(|b| &*b.source != rel);
                if list.is_empty() {
                    self.backlinks.remove(&d);
                }
            }
        }
        for m in missing {
            if let Some(list) = self.unresolved.get_mut(&m) {
                list.retain(|s| s != rel);
                if list.is_empty() {
                    self.unresolved.remove(&m);
                }
            }
        }
        self.linking.remove(rel);
        let Some(note) = self.notes.get(rel) else {
            return;
        };
        if note.parsed.links.is_empty() {
            return;
        }
        let (found, missing, any) = self.note_links(note);
        let found: Vec<(String, Backlink)> =
            found.into_iter().map(|(d, b)| (d.to_string(), b)).collect();
        let missing: Vec<String> = missing.into_iter().map(str::to_string).collect();
        // 全体を作り直したときと同じ順（リンク元のパス順、同じリンク元の中はリンクの順）に入れる
        for (dst, b) in found {
            let list = self.backlinks.entry(dst).or_default();
            let at = list.partition_point(|x| &*x.source <= rel);
            list.insert(at, b);
        }
        for m in missing {
            let list = self.unresolved.entry(m).or_default();
            let at = list.partition_point(|s| s.as_str() < rel);
            list.insert(at, rel.to_string());
        }
        if any {
            self.linking.insert(rel.to_string());
        }
    }

    /// 差し替える前のノートのリンク先（[`Vault::relink`] で古い分を消すのに使う）。
    fn old_targets(&self, rel: &str) -> (Vec<String>, Vec<String>) {
        let Some(note) = self.notes.get(rel) else {
            return Default::default();
        };
        let (found, missing, _) = self.note_links(note);
        let mut dsts: Vec<String> = found.into_iter().map(|(d, _)| d.to_string()).collect();
        dsts.sort();
        dsts.dedup();
        (dsts, missing.into_iter().map(str::to_string).collect())
    }

    /// ノートを差し替えたあとの索引の更新。名前や別名が変わらなければ、そのノートの分だけ直す。
    /// `old` は差し替える前に [`Vault::old_targets`] で求めたもの。
    fn reindex_note(&mut self, rel: &str, old: Option<OldNote>) {
        let same_names = match (&old, self.notes.get(rel)) {
            (Some((aliases, _)), Some(note)) => *aliases == note.parsed.aliases,
            _ => false,
        };
        if let (true, Some((_, targets))) = (same_names, old) {
            self.relink(rel, targets);
        } else {
            // ノートの追加・削除や別名の変更は、他のノートのリンクの解決結果も変える
            self.rebuild_links();
        }
    }

    // ---- 参照系 ----

    pub fn notes(&self) -> impl Iterator<Item = &Note> {
        self.notes.values()
    }

    pub fn note_count(&self) -> usize {
        self.notes.len()
    }

    pub fn note(&self, rel: &str) -> Option<&Note> {
        self.notes.get(rel)
    }

    /// ノート以外のファイル（画像など）
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(String::as_str)
    }

    pub fn dirs(&self) -> impl Iterator<Item = &str> {
        self.dirs.iter().map(String::as_str)
    }

    fn pick<'a>(candidates: &'a [String], from: &str) -> Option<&'a str> {
        let from_dir = parent_dir(from);
        candidates
            .iter()
            .min_by_key(|p| (parent_dir(p) != from_dir, p.len(), p.as_str()))
            .map(String::as_str)
    }

    /// リンク先文字列をノートのパスに解決する（Obsidianと同じ「最短一致」方式）。
    ///
    /// 1. リンク元からの相対パス 2. vaultルートからのパス 3. パス末尾一致
    /// 4. ファイル名一致（同じフォルダ優先）5. frontmatterのaliases
    pub fn resolve(&self, target: &str, from: &str) -> Option<&str> {
        let t = target.trim();
        let owned;
        let t = if t.contains('\\') {
            owned = t.replace('\\', "/");
            owned.as_str()
        } else {
            t
        };
        let t = strip_note_ext(t);
        if t.is_empty() {
            return None;
        }
        let lower = t.to_lowercase();
        if !lower.contains('/') && !lower.starts_with('.') {
            // 名前だけのリンク（ほとんどがこれ）。同じフォルダを優先する pick は、相対パス・ルートからの
            // 解決と同じ結果になるので、それらを飛ばして名前の索引だけで引く
            if let Some(list) = self.by_name.get(&lower) {
                return Self::pick(list, from);
            }
            return self
                .by_alias
                .get(&lower)
                .and_then(|list| Self::pick(list, from));
        }
        if let Some(joined) = join_normalized(parent_dir(from), &lower) {
            if let Some(p) = self.lower_paths.get(&joined.to_lowercase()) {
                return Some(p);
            }
        }
        let from_root = lower.trim_start_matches("./").trim_start_matches('/');
        if let Some(p) = self.lower_paths.get(from_root) {
            return Some(p);
        }
        if from_root.contains('/') {
            // パスの末尾一致。ファイル名の索引で候補を絞ってから比べる（全ノートを走査しない）
            let suffix = format!("/{from_root}");
            let matches: Vec<String> = self
                .by_name
                .get(file_name(from_root))
                .into_iter()
                .flatten()
                .filter(|p| strip_note_ext(p).to_lowercase().ends_with(&suffix))
                .cloned()
                .collect();
            if let Some(best) = Self::pick(&matches, from) {
                return self.notes.get(best).map(|n| n.path.as_str());
            }
            return None;
        }
        if let Some(list) = self.by_name.get(from_root) {
            return Self::pick(list, from);
        }
        self.by_alias
            .get(from_root)
            .and_then(|list| Self::pick(list, from))
    }

    /// 埋め込み画像など、ノート以外のファイルへのリンクを解決する。
    pub fn resolve_file(&self, target: &str, from: &str) -> Option<&str> {
        let t = target.trim().replace('\\', "/");
        if let Some(joined) = join_normalized(parent_dir(from), &t) {
            if let Some(p) = self.files.get(&joined) {
                return Some(p);
            }
        }
        if let Some(p) = self.files.get(t.trim_start_matches('/')) {
            return Some(p);
        }
        self.file_names
            .get(&file_name(&t).to_lowercase())
            .and_then(|list| Self::pick(list, from))
    }

    pub fn backlinks(&self, rel: &str) -> &[Backlink] {
        self.backlinks.get(rel).map_or(&[], Vec::as_slice)
    }

    /// ノートの指定行（前後の空白を除く）。バックリンクの文脈表示用。
    pub fn line(&self, rel: &str, line: usize) -> &str {
        self.notes
            .get(rel)
            .and_then(|n| n.text.lines().nth(line))
            .map_or("", str::trim)
    }

    /// ノートから出ているリンクと、その解決先。
    pub fn outgoing(&self, rel: &str) -> Vec<(&Link, Option<&str>)> {
        let Some(note) = self.notes.get(rel) else {
            return Vec::new();
        };
        note.parsed
            .links
            .iter()
            .map(|l| (l, self.resolve(&l.target, rel)))
            .collect()
    }

    /// リンク先が存在しないリンク（リンク先文字列 → リンク元ノート）
    pub fn unresolved(&self) -> &BTreeMap<String, Vec<String>> {
        &self.unresolved
    }

    /// このノートの名前（またはaliases）が、リンクにならずに本文中に書かれている箇所。
    /// コードブロック・インラインコード・frontmatter・既存のリンクの中は数えない。
    /// 短すぎる名前（英数字は3文字未満、それ以外は2文字未満）は誤検出が多いので対象外。
    pub fn unlinked_mentions(&self, rel: &str) -> Vec<Mention> {
        const MAX: usize = 300;
        let Some(target) = self.notes.get(rel) else {
            return Vec::new();
        };
        let long_enough = |t: &str| {
            let n = t.chars().count();
            if t.is_ascii() {
                n >= 3
            } else {
                n >= 2
            }
        };
        let mut terms: Vec<String> = std::iter::once(target.name.clone())
            .chain(target.parsed.aliases.iter().cloned())
            .filter(|t| long_enough(t.trim()))
            .map(|t| t.trim().to_string())
            .collect();
        // 長い語を先に試し、「Rust」と「Rust Notes」の両方に当たる場合は長い方を採る
        terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
        terms.dedup();
        if terms.is_empty() {
            return Vec::new();
        }
        let pattern = terms
            .iter()
            .map(|t| regex::escape(t))
            .collect::<Vec<_>>()
            .join("|");
        let Ok(re) = regex::RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .build()
        else {
            return Vec::new();
        };
        let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');

        let notes: Vec<&Note> = self.notes.values().filter(|n| n.path != rel).collect();
        let mut out: Vec<Mention> = notes
            .par_iter()
            .flat_map_iter(|note| {
                let text = &note.text;
                let links: Vec<std::ops::Range<usize>> = note
                    .parsed
                    .links
                    .iter()
                    .map(|l| mention_guard(text, l))
                    .collect();
                let mut found = Vec::new();
                let mut offset = 0;
                let mut fence = false;
                for (line_no, raw) in text.split_inclusive('\n').enumerate() {
                    let start = offset;
                    offset += raw.len();
                    if start < note.parsed.body_start {
                        continue;
                    }
                    let t = raw.trim_start();
                    if t.starts_with("```") || t.starts_with("~~~") {
                        fence = !fence;
                        continue;
                    }
                    if fence {
                        continue;
                    }
                    let line = raw.trim_end_matches(['\n', '\r']);
                    let code = inline_code_ranges(line);
                    for m in re.find_iter(line) {
                        let (a, b) = (start + m.start(), start + m.end());
                        let inside = |r: &std::ops::Range<usize>| r.start < b && a < r.end;
                        if links.iter().any(inside)
                            || code.iter().any(|r| r.start < m.end() && m.start() < r.end)
                        {
                            continue;
                        }
                        // 英数字の語は、単語の途中に当たったものを除く
                        let ascii = m.as_str().is_ascii();
                        if ascii
                            && (word(line[..m.start()].chars().next_back())
                                || word(line[m.end()..].chars().next()))
                        {
                            continue;
                        }
                        found.push(Mention {
                            source: note.path.clone(),
                            line: line_no,
                            range: a..b,
                            text: m.as_str().to_string(),
                            context: line.trim().to_string(),
                            column: m.start() - (line.len() - line.trim_start().len()),
                        });
                    }
                }
                found
            })
            .collect();
        out.sort_by(|a, b| {
            a.source
                .cmp(&b.source)
                .then(a.range.start.cmp(&b.range.start))
        });
        out.truncate(MAX);
        out
    }

    /// 未リンクの言及を `[[リンク]]` に書き換えた本文を返す（書き込みはしない）。
    /// 本文がその後変わっていて、範囲の文字列が一致しなければ None。
    pub fn link_mention_text(&self, text: &str, mention: &Mention, target: &str) -> Option<String> {
        if text.get(mention.range.clone()) != Some(mention.text.as_str()) {
            return None;
        }
        let note = self.notes.get(target)?;
        let name = if self.resolve(&note.name, &mention.source) == Some(target) {
            note.name.clone()
        } else {
            strip_note_ext(target).to_string()
        };
        let link = if mention.text == name {
            format!("[[{name}]]")
        } else {
            format!("[[{name}|{}]]", mention.text)
        };
        let mut out = text.to_string();
        out.replace_range(mention.range.clone(), &link);
        Some(out)
    }

    /// どこからもリンクされておらず、どこにもリンクしていないノート
    pub fn orphans(&self) -> Vec<&str> {
        self.notes
            .values()
            .filter(|n| self.backlinks(&n.path).is_empty() && !self.linking.contains(&n.path))
            .map(|n| n.path.as_str())
            .collect()
    }

    /// タグ → そのタグを持つノート
    pub fn tags(&self) -> BTreeMap<&str, Vec<&str>> {
        let mut map: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for n in self.notes.values() {
            for t in &n.parsed.tags {
                map.entry(t.as_str()).or_default().push(&n.path);
            }
        }
        map
    }

    /// 解決済みリンクの辺（重複なし）
    pub fn edges(&self) -> Vec<(&str, &str)> {
        let mut set = BTreeSet::new();
        for n in self.notes.values() {
            for l in &n.parsed.links {
                if let Some(dst) = self.resolve(&l.target, &n.path) {
                    if dst != n.path {
                        set.insert((n.path.as_str(), dst));
                    }
                }
            }
        }
        set.into_iter().collect()
    }

    // ---- 更新系 ----

    /// vault外に出るパスや隠しファイルを拒否する。
    fn checked(&self, rel: &str) -> Result<PathBuf> {
        if rel.is_empty() {
            bail!("empty path");
        }
        for seg in rel.split('/') {
            if seg.is_empty() || seg == "." || seg == ".." || seg.contains(['\\', ':']) {
                bail!("invalid path: {rel}");
            }
            if seg.starts_with('.') {
                bail!("hidden paths are not managed by the vault: {rel}");
            }
        }
        Ok(self.root.join(rel))
    }

    /// ディスク上のノートの現在の内容（索引ではなく実ファイルを読む）。無ければ None。
    pub fn disk_text(&self, rel: &str) -> Option<String> {
        let bytes = fs::read(self.checked(rel).ok()?).ok()?;
        Some(
            String::from_utf8(bytes)
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
        )
    }

    /// ノートを保存する（アトミック書き込み）。上書きされる内容はローカル履歴に残す。
    pub fn write_note(&mut self, rel: &str, text: &str) -> Result<()> {
        let abs = self.checked(rel)?;
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Some(old) = self.disk_text(rel).filter(|old| old != text) {
            // 履歴が残せなくても保存自体は止めない
            let _ = history::snapshot_with(&self.root, rel, &old, self.history_limits);
        }
        atomic_write(&abs, text.as_bytes()).with_context(|| format!("failed to save {rel}"))?;
        self.register_dirs(rel);
        let old = self
            .notes
            .get(rel)
            .map(|n| (n.parsed.aliases.clone(), self.old_targets(rel)));
        self.notes.insert(
            rel.to_string(),
            make_note(rel, text.to_string(), mtime_secs(&abs)),
        );
        self.reindex_note(rel, old);
        Ok(())
    }

    fn register_dirs(&mut self, rel: &str) {
        let mut dir = parent_dir(rel);
        while !dir.is_empty() && self.dirs.insert(dir.to_string()) {
            dir = parent_dir(dir);
        }
    }

    /// 名前またはパスから新規ノートを作る。`.md` は自動で補う。作成したパスを返す。
    pub fn create_note(&mut self, name: &str, content: &str) -> Result<String> {
        let mut rel = name.trim().replace('\\', "/").trim_matches('/').to_string();
        if !is_note_path(&rel) {
            rel.push_str(".md");
        }
        let abs = self.checked(&rel)?;
        if abs.exists() {
            bail!("already exists: {rel}");
        }
        self.write_note(&rel, content)?;
        Ok(rel)
    }

    pub fn create_dir(&mut self, rel: &str) -> Result<()> {
        let rel = rel.trim().replace('\\', "/");
        let rel = rel.trim_matches('/');
        let abs = self.checked(rel)?;
        fs::create_dir_all(abs)?;
        self.register_dirs(&format!("{rel}/_"));
        Ok(())
    }

    /// ノート・添付ファイル・フォルダを移動/改名し、vault内のリンクを追従させる。
    /// 書き換えたノートのパス（移動後）を返す。
    pub fn rename(&mut self, old: &str, new: &str) -> Result<Vec<String>> {
        let old = old.trim_matches('/');
        let mut new = new.trim().replace('\\', "/").trim_matches('/').to_string();
        let old_abs = self.checked(old)?;
        if !old_abs.exists() {
            bail!("not found: {old}");
        }
        let is_dir = old_abs.is_dir();
        if !is_dir && is_note_path(old) && !is_note_path(&new) {
            new.push_str(".md");
        }
        let new_abs = self.checked(&new)?;
        if old == new {
            return Ok(Vec::new());
        }
        if new_abs.exists() && old.to_lowercase() != new.to_lowercase() {
            bail!("already exists: {new}");
        }
        if is_dir && new.starts_with(&format!("{old}/")) {
            bail!("cannot move a folder into itself");
        }

        // 旧パス → 新パス
        let moved = |p: &str| -> Option<String> {
            if p == old {
                Some(new.clone())
            } else if is_dir {
                p.strip_prefix(old)
                    .filter(|rest| rest.starts_with('/'))
                    .map(|rest| format!("{new}{rest}"))
            } else {
                None
            }
        };

        let rewrites: Vec<(String, String)> = self
            .retarget(&moved)
            .into_iter()
            .map(|(_, path, text)| (path, text))
            .collect();

        if let Some(parent) = new_abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&old_abs, &new_abs)
            .with_context(|| format!("failed to rename {old} -> {new}"))?;
        history::rename(&self.root, old, &new);

        let mut updated = Vec::new();
        for (path, text) in rewrites {
            atomic_write(&self.root.join(&path), text.as_bytes())
                .with_context(|| format!("failed to update links in {path}"))?;
            updated.push(path);
        }
        self.rescan();
        Ok(updated)
    }

    /// ノートやファイルの移動（`moved`: 旧パス → 新パス）に合わせて、リンクを書き換えた本文を作る。
    /// 書き換えが要るノートごとに (元のパス, 移動後のパス, 書き換えた本文)。書き込みはしない。
    fn retarget(&self, moved: &dyn Fn(&str) -> Option<String>) -> Vec<(String, String, String)> {
        // 旧索引でリンクを解決しながら、書き換え後の本文を作る
        let mut rewrites = Vec::new();
        for note in self.notes.values() {
            let src_new = moved(&note.path);
            let src_final = src_new.as_deref().unwrap_or(&note.path);
            let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
            for link in &note.parsed.links {
                let (dst, dst_is_note) = match self.resolve(&link.target, &note.path) {
                    Some(d) => (d, true),
                    None => match self.resolve_file(&link.target, &note.path) {
                        Some(d) => (d, false),
                        None => continue,
                    },
                };
                let dst_new = moved(dst);
                let dst_final = dst_new.as_deref().unwrap_or(dst);
                let replacement = match link.kind {
                    LinkKind::Wiki => {
                        if dst_new.is_none() {
                            continue;
                        }
                        let shown = if dst_is_note {
                            strip_note_ext(dst_final)
                        } else {
                            dst_final
                        };
                        if link.target.contains('/') {
                            shown.to_string()
                        } else {
                            file_name(shown).to_string()
                        }
                    }
                    LinkKind::Markdown => {
                        if dst_new.is_none() && src_new.is_none() {
                            continue;
                        }
                        parse::percent_encode_path(&relative_path(parent_dir(src_final), dst_final))
                    }
                };
                if note.text[link.span.clone()] != replacement {
                    edits.push((link.span.clone(), replacement));
                }
            }
            if edits.is_empty() {
                continue;
            }
            let mut text = note.text.clone();
            for (span, rep) in edits.into_iter().rev() {
                text.replace_range(span, &rep);
            }
            rewrites.push((note.path.clone(), src_final.to_string(), text));
        }

        rewrites
    }

    /// `source` の本文を `target` の末尾に足し、`source` へのリンクを `target` に付け替える
    /// （ノートコンポーザーの「結合」）。`source` のファイルは消さないので、呼び出し側でゴミ箱へ移す。
    /// 書き換えたノートのパスを返す。
    pub fn merge(&mut self, source: &str, target: &str) -> Result<Vec<String>> {
        if source == target {
            bail!("同じノートには結合できません");
        }
        let (Some(src), Some(dst)) = (self.notes.get(source), self.notes.get(target)) else {
            bail!("ノートが見つかりません");
        };
        let target_text = dst.text.clone();
        let source_text = src.text.clone();
        let (source_path, target_path) = (source.to_string(), target.to_string());
        let moved = move |p: &str| (p == source_path).then(|| target_path.clone());
        let mut source_body = source_text;
        let mut target_body = target_text;
        let mut updated = Vec::new();
        let mut others = Vec::new();
        for (orig, _, text) in self.retarget(&moved) {
            if orig == source {
                // 結合元の本文（相対パスのリンクは結合先の場所に合わせて書き換え済み）
                source_body = text;
            } else if orig == target {
                target_body = text;
            } else {
                others.push((orig, text));
            }
        }
        // 結合元の frontmatter は、本文の途中に入らないよう外す
        if let Some(fm) = crate::props::block(&source_body) {
            source_body = source_body[fm.end..]
                .trim_start_matches(['\n', '\r'])
                .to_string();
        }
        let mut merged = target_body.trim_end().to_string();
        if !merged.is_empty() {
            merged.push_str("\n\n");
        }
        merged.push_str(&source_body);
        if !merged.ends_with('\n') {
            merged.push('\n');
        }
        self.write_note(target, &merged)?;
        updated.push(target.to_string());
        for (path, text) in others {
            self.write_note(&path, &text)?;
            updated.push(path);
        }
        Ok(updated)
    }

    /// タグを改名する。本文の `#古い` と `#古い/子`、frontmatterの tags を書き換える。
    /// 書き換えたノートのパスを返す。
    pub fn rename_tag(&mut self, old: &str, new: &str) -> Result<Vec<String>> {
        let old = old.trim().trim_start_matches('#');
        let new = new.trim().trim_start_matches('#');
        if old.is_empty() || new.is_empty() {
            bail!("tag name is empty");
        }
        if new.contains(|c: char| c.is_whitespace() || c == '#') {
            bail!("invalid tag name: {new}");
        }
        let lower = old.to_lowercase();
        // 大文字小文字は区別しない（Obsidianと同じ）。子タグは残りの部分をそのまま保つ
        let rename = |tag: &str| -> Option<String> {
            let t = tag.to_lowercase();
            if t == lower {
                Some(new.to_string())
            } else if t.starts_with(&format!("{lower}/")) {
                Some(format!("{new}{}", &tag[old.len()..]))
            } else {
                None
            }
        };
        let mut changes: Vec<(String, String)> = Vec::new();
        for note in self.notes.values() {
            let mut text = note.text.clone();
            for span in note.parsed.tag_spans.iter().rev() {
                if let Some(rep) = rename(&note.text[span.clone()]) {
                    text.replace_range(span.clone(), &rep);
                }
            }
            if let Some(props) = crate::props::read(&text) {
                for p in props
                    .iter()
                    .filter(|p| matches!(p.key.as_str(), "tags" | "tag"))
                {
                    let value = match &p.value {
                        crate::props::Value::List(items) => {
                            let renamed: Vec<String> = items
                                .iter()
                                .map(|t| {
                                    rename(t.trim_start_matches('#')).unwrap_or_else(|| t.clone())
                                })
                                .collect();
                            (renamed != *items).then_some(crate::props::Value::List(renamed))
                        }
                        // `tags: a b` や `tags: a, b` のような1行の書き方
                        crate::props::Value::Text(t) => {
                            let parts: Vec<&str> =
                                t.split([' ', ',']).filter(|s| !s.is_empty()).collect();
                            let renamed: Vec<String> = parts
                                .iter()
                                .map(|s| {
                                    rename(s.trim_start_matches('#'))
                                        .unwrap_or_else(|| s.to_string())
                                })
                                .collect();
                            let changed = renamed.iter().zip(&parts).any(|(a, b)| a != b);
                            changed.then(|| crate::props::Value::Text(renamed.join(", ")))
                        }
                        crate::props::Value::Raw(_) => None,
                    };
                    if let Some(v) = value {
                        text = crate::props::set(&text, &p.key, &v);
                    }
                }
            }
            if text != note.text {
                changes.push((note.path.clone(), text));
            }
        }
        let mut changed = Vec::new();
        for (path, text) in changes {
            self.write_note(&path, &text)?;
            changed.push(path);
        }
        Ok(changed)
    }

    /// ディスクの状態に合わせて、変わったノートだけを読み直す（更新日時で判定）。
    /// 添付ファイルやフォルダが増減していれば全体を読み直す。読み直したノートの数を返す。
    pub fn refresh(&mut self) -> usize {
        let mut seen: HashMap<String, u64> = HashMap::new();
        let mut files = BTreeSet::new();
        let walker = WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
        for entry in walker.filter_map(|e| e.ok()) {
            if entry.depth() == 0 || entry.file_type().is_dir() {
                continue;
            }
            let Some(rel) = self.rel(entry.path()) else {
                continue;
            };
            if is_note_path(&rel) {
                seen.insert(rel, mtime_secs(entry.path()));
            } else {
                files.insert(rel);
            }
        }
        if files != self.files {
            self.rescan();
            return self.notes.len();
        }
        let mut stale: Vec<String> = seen
            .iter()
            .filter(|(rel, mtime)| self.notes.get(*rel).is_none_or(|n| n.mtime != **mtime))
            .map(|(rel, _)| rel.clone())
            .collect();
        stale.extend(
            self.notes
                .keys()
                .filter(|rel| !seen.contains_key(*rel))
                .cloned(),
        );
        stale.sort();
        let mut count = 0;
        for rel in stale {
            if self.reload(&rel) {
                count += 1;
            }
        }
        count
    }

    // ---- デイリーノート ----

    /// `.obsidian/<file>` の文字列設定を読む（空文字は無いものとして扱う）。
    fn obsidian_setting(&self, file: &str, key: &str) -> Option<String> {
        let text = fs::read_to_string(self.root.join(".obsidian").join(file)).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let s = value.get(key)?.as_str()?.trim().trim_matches('/');
        (!s.is_empty()).then(|| s.to_string())
    }

    /// プロパティの型（`.obsidian/types.json`）。指定の無いプロパティは含まない
    pub fn property_types(&self) -> BTreeMap<String, crate::props::PropType> {
        let Ok(text) = fs::read_to_string(self.root.join(".obsidian").join("types.json")) else {
            return BTreeMap::new();
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return BTreeMap::new();
        };
        value
            .get("types")
            .and_then(|t| t.as_object())
            .map(|types| {
                types
                    .iter()
                    .filter_map(|(k, v)| {
                        Some((
                            k.clone(),
                            crate::props::PropType::from_obsidian(v.as_str()?)?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// プロパティの型を `.obsidian/types.json` に書く（ほかの項目はそのまま残す）
    pub fn set_property_type(&self, key: &str, ty: crate::props::PropType) -> Result<()> {
        let dir = self.root.join(".obsidian");
        let path = dir.join("types.json");
        let mut value = fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .filter(|v| v.is_object())
            .unwrap_or_else(|| serde_json::json!({}));
        let types = value
            .as_object_mut()
            .expect("上でオブジェクトに限った")
            .entry("types")
            .or_insert_with(|| serde_json::json!({}));
        if !types.is_object() {
            *types = serde_json::json!({});
        }
        types
            .as_object_mut()
            .expect("上でオブジェクトにした")
            .insert(key.to_string(), ty.obsidian_name().into());
        fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(&value)?;
        atomic_write(&path, json.as_bytes()).context("failed to save .obsidian/types.json")
    }

    /// vault のノートで使われているプロパティの名前（多い順）
    pub fn property_keys(&self) -> Vec<String> {
        let mut count: HashMap<String, usize> = HashMap::new();
        for note in self.notes.values() {
            for p in crate::props::read(&note.text).unwrap_or_default() {
                *count.entry(p.key).or_default() += 1;
            }
        }
        let mut keys: Vec<(String, usize)> = count.into_iter().collect();
        keys.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        keys.into_iter().map(|(k, _)| k).collect()
    }

    /// ノートに貼り付ける添付ファイルの保存先パス（vault相対）。Obsidianの設定
    /// （`.obsidian/app.json` の attachmentFolderPath）に従い、同名があれば番号を付ける。
    /// 設定が無ければvaultのルート。`./` で始まる設定はノートと同じフォルダからの相対。
    pub fn attachment_path(&self, note: &str, name: &str) -> String {
        let setting = self.obsidian_setting("app.json", "attachmentFolderPath");
        let folder = match setting.as_deref() {
            None => String::new(),
            Some(".") => parent_dir(note).to_string(),
            Some(s) if s.starts_with("./") => {
                join_normalized(parent_dir(note), &s[2..]).unwrap_or_default()
            }
            Some(s) => s.to_string(),
        };
        let join = |file: &str| {
            if folder.is_empty() {
                file.to_string()
            } else {
                format!("{folder}/{file}")
            }
        };
        let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
        let mut candidate = join(name);
        let mut n = 1;
        while self.abs(&candidate).exists() {
            candidate = join(&format!("{stem} {n}.{ext}"));
            n += 1;
        }
        candidate
    }

    /// 添付ファイル（ノート以外）を書き込む。
    pub fn write_file(&mut self, rel: &str, bytes: &[u8]) -> Result<()> {
        let abs = self.checked(rel)?;
        if is_note_path(rel) {
            bail!("use write_note for notes: {rel}");
        }
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write(&abs, bytes).with_context(|| format!("failed to save {rel}"))?;
        self.register_dirs(rel);
        self.files.insert(rel.to_string());
        self.rebuild_links();
        Ok(())
    }

    /// テンプレートを置くフォルダ。Obsidianの設定（`.obsidian/templates.json` の folder）、
    /// 無ければ `templates`。
    pub fn template_folder(&self) -> String {
        self.obsidian_setting("templates.json", "folder")
            .unwrap_or_else(|| "templates".to_string())
    }

    /// テンプレートフォルダ内のノート（パス順）。
    pub fn templates(&self) -> Vec<&str> {
        let prefix = format!("{}/", self.template_folder());
        self.notes
            .keys()
            .filter(|p| p.starts_with(&prefix))
            .map(String::as_str)
            .collect()
    }

    /// 名前（テンプレートフォルダからの相対パス、拡張子なしでも可）またはパスでテンプレートを探す。
    pub fn find_template(&self, name: &str) -> Option<&str> {
        let name = name.trim().trim_end_matches(".md");
        let folder = self.template_folder();
        self.templates().into_iter().find(|p| {
            let stem = p.trim_end_matches(".md");
            stem == name
                || stem
                    .strip_prefix(&folder)
                    .map(|s| s.trim_start_matches('/'))
                    == Some(name)
                || file_name(stem) == name
        })
    }

    /// テンプレートを展開した本文。`title` は作るノートの名前。
    pub fn render_template(&self, template: &str, title: &str) -> Option<String> {
        let text = &self.notes.get(template)?.text;
        Some(template::render(text, title, &chrono::Local::now()))
    }

    /// 指定日のデイリーノートのパス。Obsidianの設定
    /// （`.obsidian/daily-notes.json` の folder / format）があればそれに従う。
    pub fn daily_path(&self, date: chrono::NaiveDate) -> String {
        let format = self
            .obsidian_setting("daily-notes.json", "format")
            .unwrap_or_else(|| "YYYY-MM-DD".to_string());
        let name = template::format_moment(&date.and_time(chrono::NaiveTime::MIN), &format);
        match self.obsidian_setting("daily-notes.json", "folder") {
            Some(folder) => format!("{folder}/{name}.md"),
            None => format!("{name}.md"),
        }
    }

    /// 今日のデイリーノートを（無ければ作って）返す。作るときは、Obsidianの設定で
    /// 指定されたテンプレート（`.obsidian/daily-notes.json` の template）を展開する。
    pub fn daily(&mut self) -> Result<String> {
        let rel = self.daily_path(chrono::Local::now().date_naive());
        if !self.notes.contains_key(&rel) {
            if self.abs(&rel).exists() {
                self.reload(&rel);
            } else {
                let title = strip_note_ext(file_name(&rel)).to_string();
                let body = self
                    .obsidian_setting("daily-notes.json", "template")
                    .and_then(|t| self.find_template(&t).map(str::to_string))
                    .and_then(|t| self.render_template(&t, &title))
                    .unwrap_or_default();
                self.write_note(&rel, &body)?;
            }
        }
        Ok(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(files: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let abs = dir.path().join(path);
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, text).unwrap();
        }
        let v = Vault::open(dir.path()).unwrap();
        (dir, v)
    }

    #[test]
    fn scan_and_resolve() {
        let (_d, v) = vault(&[
            ("a.md", "[[B]] [[sub/c]] [[missing]] ![[pic.png]]"),
            ("B.md", "---\naliases: [Bee]\n---\n[[a]]"),
            ("sub/c.md", "[up](../a.md) [[Bee]]"),
            ("sub/b.md", "[[b]]"),
            ("img/pic.png", ""),
            (".obsidian/x.md", "[[a]]"),
        ]);
        assert_eq!(v.note_count(), 4);
        assert_eq!(v.resolve("b", "a.md"), Some("B.md"));
        // 同じフォルダのノートを優先
        assert_eq!(v.resolve("b", "sub/c.md"), Some("sub/b.md"));
        assert_eq!(v.resolve("Bee", "sub/c.md"), Some("B.md"));
        assert_eq!(v.resolve_file("pic.png", "a.md"), Some("img/pic.png"));
        let sources: Vec<&str> = v.backlinks("a.md").iter().map(|b| &*b.source).collect();
        assert_eq!(sources, ["B.md", "sub/c.md"]);
        assert_eq!(v.unresolved().keys().collect::<Vec<_>>(), ["missing"]);
        assert!(v.orphans().is_empty());
    }

    /// 差分更新した索引が、全体を作り直した索引と一致することを確かめる。
    fn assert_index_matches_fresh(v: &Vault) {
        let fresh = Vault::open(v.root()).unwrap();
        let mut keys: Vec<&String> = fresh.backlinks.keys().chain(v.backlinks.keys()).collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            assert_eq!(v.backlinks(k), fresh.backlinks(k), "backlinks of {k}");
        }
        assert_eq!(v.unresolved, fresh.unresolved);
        assert_eq!(v.orphans(), fresh.orphans());
        assert_eq!(v.linking, fresh.linking);
    }

    #[test]
    fn incremental_index_matches_full_rebuild() {
        let (d, mut v) = vault(&[
            ("a.md", "[[b]] [[c]] [[missing]]"),
            ("b.md", "[[a]]\n[[a]] [[c]]"),
            ("c.md", "nothing"),
            ("sub/d.md", "[[a]] [[sub/e]]"),
            ("sub/e.md", "---\naliases: [Echo]\n---\n[[d]]"),
        ]);
        assert_index_matches_fresh(&v);
        // 既存ノートの書き換え（差分更新）
        v.write_note("c.md", "[[a]] [[b]] [[gone]]").unwrap();
        assert_index_matches_fresh(&v);
        v.write_note("a.md", "no links now").unwrap();
        assert_index_matches_fresh(&v);
        v.write_note("b.md", "[[Echo]] [[c]]\n[[missing]]").unwrap();
        assert_index_matches_fresh(&v);
        // 別名の変更・新しいノート・削除は全体の作り直し
        v.write_note("sub/e.md", "---\naliases: [Other]\n---\n")
            .unwrap();
        assert_index_matches_fresh(&v);
        v.write_note("missing.md", "[[a]]").unwrap();
        assert_index_matches_fresh(&v);
        fs::remove_file(d.path().join("c.md")).unwrap();
        v.reload("c.md");
        assert_index_matches_fresh(&v);
        fs::write(d.path().join("b.md"), "[[a]] [[sub/d]]").unwrap();
        v.reload("b.md");
        assert_index_matches_fresh(&v);
    }

    #[test]
    fn write_create_reload() {
        let (d, mut v) = vault(&[("a.md", "x")]);
        let rel = v.create_note("dir/New Note", "[[a]] #t").unwrap();
        assert_eq!(rel, "dir/New Note.md");
        assert!(v.create_note("dir/New Note", "").is_err());
        assert!(v.create_note("../evil", "").is_err());
        assert_eq!(v.backlinks("a.md").len(), 1);
        assert_eq!(v.tags()["t"], ["dir/New Note.md"]);
        assert!(v.dirs().any(|p| p == "dir"));

        fs::write(d.path().join("a.md"), "[[New Note]]").unwrap();
        assert!(v.reload("a.md"));
        assert!(!v.reload("a.md"));
        assert_eq!(v.backlinks("dir/New Note.md").len(), 1);
        fs::remove_file(d.path().join("a.md")).unwrap();
        assert!(v.reload("a.md"));
        assert!(v.note("a.md").is_none());
    }

    #[test]
    fn merge_moves_text_and_links() {
        let dir = tempfile::tempdir().unwrap();
        let w = |p: &str, t: &str| {
            let abs = dir.path().join(p);
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, t).unwrap();
        };
        w(
            "a/src.md",
            "---\ntags: [x]\n---\n# 元\n画像 ![](pic.png) と [[target]] と [[src]]\n",
        );
        w("a/pic.png", "png");
        w("b/target.md", "# 先\n本文\n");
        w(
            "other.md",
            "[[src]] / [[src|別名]] / [[src#元]] / [md](a/src.md)\n",
        );
        let mut v = Vault::open(dir.path()).unwrap();
        let updated = v.merge("a/src.md", "b/target.md").unwrap();
        assert_eq!(updated, ["b/target.md", "other.md"]);
        let target = fs::read_to_string(dir.path().join("b/target.md")).unwrap();
        // 結合元の frontmatter は外し、相対パスのリンクは結合先の場所から直す
        assert_eq!(
            target,
            "# 先\n本文\n\n# 元\n画像 ![](../a/pic.png) と [[target]] と [[target]]\n"
        );
        let other = fs::read_to_string(dir.path().join("other.md")).unwrap();
        assert_eq!(
            other,
            "[[target]] / [[target|別名]] / [[target#元]] / [md](b/target.md)\n"
        );
        // 結合元のファイルは呼び出し側で消す
        assert!(dir.path().join("a/src.md").exists());
        assert!(v.merge("b/target.md", "b/target.md").is_err());
    }

    #[test]
    fn rename_updates_links() {
        let (d, mut v) = vault(&[
            (
                "a.md",
                "[[old]] [[old#h|alias]] [m](notes/old.md) ![[pic.png]]",
            ),
            ("notes/old.md", "[[a]] [rel](../a.md)"),
            ("pic.png", ""),
        ]);
        let updated = v.rename("notes/old.md", "archive/New Name").unwrap();
        assert_eq!(updated, ["a.md"]);
        let a = fs::read_to_string(d.path().join("a.md")).unwrap();
        assert_eq!(
            a,
            "[[New Name]] [[New Name#h|alias]] [m](archive/New%20Name.md) ![[pic.png]]"
        );
        assert_eq!(
            v.note("archive/New Name.md").unwrap().text,
            "[[a]] [rel](../a.md)"
        );
        assert_eq!(v.backlinks("archive/New Name.md").len(), 1);

        v.rename("pic.png", "img/photo.png").unwrap();
        assert!(v.note("a.md").unwrap().text.ends_with("![[photo.png]]"));
    }

    #[test]
    fn overwrites_are_kept_in_history() {
        let (d, mut v) = vault(&[("a.md", "first")]);
        v.write_note("a.md", "second").unwrap();
        v.write_note("a.md", "second").unwrap();
        let versions = history::list(d.path(), "a.md");
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].read().unwrap(), "first");
        // 履歴は索引に載らない
        assert_eq!(v.note_count(), 1);
        v.rename("a.md", "b").unwrap();
        assert_eq!(history::list(d.path(), "b.md").len(), 1);
        assert_eq!(v.disk_text("b.md").as_deref(), Some("second"));
    }

    #[test]
    fn unlinked_mentions() {
        let (_d, mut v) = vault(&[
            ("Rust Notes.md", "---\naliases: [ラスト]\n---\nbody"),
            (
                "a.md",
                "I like rust notes a lot.\nAlready [[Rust Notes]] linked.\n`Rust Notes` in code\nラストの話 and TrustNotes rust notesy",
            ),
            ("b.md", "```\nRust Notes\n```\n[Rust Notes](Rust%20Notes.md)"),
            ("Go.md", "go go"),
        ]);
        let m = v.unlinked_mentions("Rust Notes.md");
        let found: Vec<(&str, usize, &str)> = m
            .iter()
            .map(|m| (m.source.as_str(), m.line, m.text.as_str()))
            .collect();
        assert_eq!(found, [("a.md", 0, "rust notes"), ("a.md", 3, "ラスト")]);
        assert_eq!(&m[0].context[m[0].column..][..10], "rust notes");
        // 短い名前は対象外
        assert!(v.unlinked_mentions("Go.md").is_empty());

        let text = v.note("a.md").unwrap().text.clone();
        let linked = v.link_mention_text(&text, &m[0], "Rust Notes.md").unwrap();
        assert!(linked.starts_with("I like [[Rust Notes|rust notes]] a lot."));
        let linked = v.link_mention_text(&linked, &m[1], "Rust Notes.md");
        assert!(linked.is_none(), "範囲がずれたら書き換えない");
        v.write_note(
            "a.md",
            &v.link_mention_text(&text, &m[1], "Rust Notes.md").unwrap(),
        )
        .unwrap();
        assert!(v
            .note("a.md")
            .unwrap()
            .text
            .contains("[[Rust Notes|ラスト]]の話"));
        assert_eq!(v.unlinked_mentions("Rust Notes.md").len(), 1);
    }

    #[test]
    fn tags_can_be_renamed() {
        let (_d, mut v) = vault(&[
            (
                "a.md",
                "---\ntags: [proj, other]\n---\n#proj と #Proj/sub と #project と `#proj`",
            ),
            ("b.md", "---\ntags: proj/x keep\n---\nbody"),
            ("c.md", "#other"),
        ]);
        let changed = v.rename_tag("#proj", "work").unwrap();
        assert_eq!(changed, ["a.md", "b.md"]);
        assert_eq!(
            v.note("a.md").unwrap().text,
            "---\ntags:\n  - work\n  - other\n---\n#work と #work/sub と #project と `#proj`"
        );
        assert!(v
            .note("b.md")
            .unwrap()
            .text
            .starts_with("---\ntags: work/x, keep\n"));
        assert!(v.tags().contains_key("work/sub"));
        assert!(v.rename_tag("x", "a b").is_err());
    }

    #[test]
    fn refresh_reloads_changed_notes() {
        let (d, mut v) = vault(&[("a.md", "x"), ("b.md", "[[a]]")]);
        assert_eq!(v.refresh(), 0);
        fs::write(d.path().join("c.md"), "[[a]]").unwrap();
        fs::remove_file(d.path().join("b.md")).unwrap();
        assert_eq!(v.refresh(), 2);
        assert!(v.note("c.md").is_some() && v.note("b.md").is_none());
        assert_eq!(v.backlinks("a.md").len(), 1);
    }

    #[test]
    fn attachments() {
        let (d, mut v) = vault(&[("notes/a.md", "")]);
        assert_eq!(v.attachment_path("notes/a.md", "p.png"), "p.png");
        v.write_file("p.png", b"x").unwrap();
        assert_eq!(v.attachment_path("notes/a.md", "p.png"), "p 1.png");
        assert_eq!(v.resolve_file("p.png", "notes/a.md"), Some("p.png"));
        assert!(v.write_file("x.md", b"").is_err());
        fs::create_dir_all(d.path().join(".obsidian")).unwrap();
        let set = |value: &str| {
            let json = format!(r#"{{"attachmentFolderPath":"{value}"}}"#);
            fs::write(d.path().join(".obsidian/app.json"), json).unwrap();
        };
        set("./");
        assert_eq!(v.attachment_path("notes/a.md", "p.png"), "notes/p.png");
        set("./img");
        assert_eq!(v.attachment_path("notes/a.md", "p.png"), "notes/img/p.png");
        set("assets");
        assert_eq!(v.attachment_path("notes/a.md", "p.png"), "assets/p.png");
    }

    #[test]
    fn rename_folder() {
        let (_d, mut v) = vault(&[
            ("a.md", "[[proj/x]] [[y]] [l](proj/y.md)"),
            ("proj/x.md", "[l](y.md) [l](../a.md)"),
            ("proj/y.md", ""),
        ]);
        v.rename("proj", "done/proj2").unwrap();
        assert_eq!(
            v.note("a.md").unwrap().text,
            "[[done/proj2/x]] [[y]] [l](done/proj2/y.md)"
        );
        assert_eq!(
            v.note("done/proj2/x.md").unwrap().text,
            "[l](y.md) [l](../../a.md)"
        );
        assert!(v.rename("done", "done/proj2/in").is_err());
    }

    #[test]
    fn daily() {
        let (d, mut v) = vault(&[("a.md", "")]);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
        assert_eq!(v.daily_path(date), "2026-01-05.md");
        fs::create_dir_all(d.path().join(".obsidian")).unwrap();
        fs::write(
            d.path().join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY/MM-DD"}"#,
        )
        .unwrap();
        assert_eq!(v.daily_path(date), "Daily/2026/01-05.md");
        let rel = v.daily().unwrap();
        assert!(v.note(&rel).is_some());

        // テンプレートの指定があれば、新しく作るデイリーノートに展開する
        fs::write(
            d.path().join(".obsidian/daily-notes.json"),
            r#"{"folder":"Daily","format":"YYYY-MM-DD","template":"tpl/日記"}"#,
        )
        .unwrap();
        fs::write(
            d.path().join(".obsidian/templates.json"),
            r#"{"folder":"tpl"}"#,
        )
        .unwrap();
        v.write_note("tpl/日記.md", "# {{title}}\n- ").unwrap();
        assert_eq!(v.templates(), ["tpl/日記.md"]);
        assert_eq!(v.find_template("日記"), Some("tpl/日記.md"));
        let rel = v.daily().unwrap();
        let title = rel.trim_start_matches("Daily/").trim_end_matches(".md");
        assert_eq!(v.note(&rel).unwrap().text, format!("# {title}\n- "));
    }

    #[test]
    fn property_types_round_trip() {
        use crate::props::PropType;
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("a.md"),
            "---\ndue: 2026-01-01\ndone: true\n---\n",
        )
        .unwrap();
        fs::write(dir.path().join("b.md"), "---\ndue: 2026-02-01\n---\n").unwrap();
        fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        fs::write(
            dir.path().join(".obsidian/types.json"),
            r#"{"types": {"done": "checkbox"}, "other": 1}"#,
        )
        .unwrap();
        let v = Vault::open(dir.path()).unwrap();
        assert_eq!(v.property_types().get("done"), Some(&PropType::Checkbox));
        v.set_property_type("due", PropType::Date).unwrap();
        let types = v.property_types();
        assert_eq!(types.get("due"), Some(&PropType::Date));
        assert_eq!(types.get("done"), Some(&PropType::Checkbox));
        // ほかの項目は残る
        let raw = fs::read_to_string(dir.path().join(".obsidian/types.json")).unwrap();
        assert!(raw.contains("\"other\": 1"), "{raw}");
        assert_eq!(v.property_keys(), ["due", "done"]);
    }
}
