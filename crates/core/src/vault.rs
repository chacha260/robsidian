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
use std::collections::{BTreeMap, BTreeSet, HashMap};
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
    pub source: String,
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
        let changed = match read_note(&self.root, rel) {
            Some(note) => {
                let same = self.notes.get(rel).is_some_and(|old| old.text == note.text);
                self.notes.insert(rel.to_string(), note);
                !same
            }
            None => self.notes.remove(rel).is_some(),
        };
        if changed {
            self.rebuild_links();
        }
        changed
    }

    fn rebuild_links(&mut self) {
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

        // リンク解決はノートごとに独立なので並列に行い、最後に順序を保って集約する
        let notes: Vec<&Note> = self.notes.values().collect();
        // (リンク元, 解決できたリンク[(リンク先, Backlink)], 解決できなかったリンク先)
        type PerNote<'a> = (&'a str, Vec<(&'a str, Backlink)>, Vec<&'a str>);
        let per_note: Vec<PerNote> = notes
            .par_iter()
            .filter(|note| !note.parsed.links.is_empty())
            .map(|note| {
                let mut found: Vec<(&str, Backlink)> = Vec::new();
                let mut missing: Vec<&str> = Vec::new();
                for link in &note.parsed.links {
                    if let Some(dst) = self.resolve(&link.target, &note.path) {
                        // 自己リンクと、同じ行からの重複リンクは数えない
                        if dst == note.path
                            || found.iter().any(|(d, b)| *d == dst && b.line == link.line)
                        {
                            continue;
                        }
                        found.push((
                            dst,
                            Backlink {
                                source: note.path.clone(),
                                line: link.line,
                            },
                        ));
                    } else if self.resolve_file(&link.target, &note.path).is_none()
                        && !missing.contains(&link.target.as_str())
                    {
                        missing.push(&link.target);
                    }
                }
                (note.path.as_str(), found, missing)
            })
            .collect();
        let mut backlinks: HashMap<String, Vec<Backlink>> = HashMap::new();
        let mut unresolved: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (source, found, missing) in per_note {
            for (dst, b) in found {
                backlinks.entry(dst.to_string()).or_default().push(b);
            }
            for target in missing {
                unresolved
                    .entry(target.to_string())
                    .or_default()
                    .push(source.to_string());
            }
        }
        self.backlinks = backlinks;
        self.unresolved = unresolved;
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
        let t = target.trim().replace('\\', "/");
        let t = strip_note_ext(&t);
        if t.is_empty() {
            return None;
        }
        let lower = t.to_lowercase();
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
            let suffix = format!("/{from_root}");
            let matches: Vec<String> = self
                .lower_paths
                .iter()
                .filter(|(k, _)| k.ends_with(&suffix))
                .map(|(_, v)| v.clone())
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
            .filter(|n| {
                self.backlinks(&n.path).is_empty()
                    && !n
                        .parsed
                        .links
                        .iter()
                        .any(|l| self.resolve(&l.target, &n.path).is_some())
            })
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
            let _ = history::snapshot(&self.root, rel, &old);
        }
        atomic_write(&abs, text.as_bytes()).with_context(|| format!("failed to save {rel}"))?;
        self.register_dirs(rel);
        self.notes.insert(
            rel.to_string(),
            make_note(rel, text.to_string(), mtime_secs(&abs)),
        );
        self.rebuild_links();
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

        // 旧索引でリンクを解決しながら、書き換え後の本文を作る
        let mut rewrites: Vec<(String, String)> = Vec::new();
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
            rewrites.push((src_final.to_string(), text));
        }

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

    // ---- デイリーノート ----

    /// `.obsidian/<file>` の文字列設定を読む（空文字は無いものとして扱う）。
    fn obsidian_setting(&self, file: &str, key: &str) -> Option<String> {
        let text = fs::read_to_string(self.root.join(".obsidian").join(file)).ok()?;
        let value: serde_json::Value = serde_json::from_str(&text).ok()?;
        let s = value.get(key)?.as_str()?.trim().trim_matches('/');
        (!s.is_empty()).then(|| s.to_string())
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
        let sources: Vec<_> = v.backlinks("a.md").iter().map(|b| &b.source).collect();
        assert_eq!(sources, ["B.md", "sub/c.md"]);
        assert_eq!(v.unresolved().keys().collect::<Vec<_>>(), ["missing"]);
        assert!(v.orphans().is_empty());
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
}
