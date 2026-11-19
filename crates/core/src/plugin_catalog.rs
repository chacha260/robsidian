//! プラグインの一覧（フェーズE3）: `catalog.json` を読み、検索し、プラグインを入れる。
//!
//! ファイルは `curl` で取ってくる（各 OS に入っていて、TLS のライブラリを足さずに済み、
//! プロキシや社内の証明書の設定もそのまま効く）。`file://` とローカルのパスも読める。
//! 入れるときは、すべてのファイルの sha256 を確かめてから、プラグインのフォルダを置き換える。
//! 入れても許可はしない（使う前に中身を見せて許可を求めるのは、ほかのプラグインと同じ）。

use crate::plugin;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileRef {
    /// 一覧からの相対、または絶対の URL
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// プラグインのフォルダの中のパス → 取ってくる場所
    pub files: BTreeMap<String, FileRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Catalog {
    pub plugins: Vec<Entry>,
}

/// URL かパスの中身を取ってくる
pub fn fetch(url: &str) -> Result<Vec<u8>> {
    if url.starts_with("http://") || url.starts_with("https://") {
        let out = Command::new("curl")
            .args(["-fsSL", "--max-time", "60", url])
            .output()
            .context("curl を起動できません（プラグインの一覧を取るのに使います）")?;
        if !out.status.success() {
            bail!(
                "取得できません: {url}（{}）",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        return Ok(out.stdout);
    }
    let path = url.strip_prefix("file://").unwrap_or(url);
    std::fs::read(path).with_context(|| format!("読めません: {path}"))
}

/// 一覧を読む
pub fn load(url: &str) -> Result<Catalog> {
    let bytes = fetch(url)?;
    let catalog: Catalog =
        serde_json::from_slice(&bytes).context("プラグインの一覧（JSON）の形が正しくありません")?;
    Ok(catalog)
}

/// 名前・id・説明に、空白で区切った語がすべて含まれるもの
pub fn search<'a>(catalog: &'a Catalog, query: &str) -> Vec<&'a Entry> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    catalog
        .plugins
        .iter()
        .filter(|e| {
            let hay = format!(
                "{} {} {}",
                e.id,
                e.name,
                e.description.as_deref().unwrap_or("")
            )
            .to_lowercase();
            words.iter().all(|w| hay.contains(w))
        })
        .collect()
}

/// `files` の URL を、一覧の場所から見た URL にする
pub fn resolve(catalog_url: &str, url: &str) -> String {
    if url.contains("://") || url.starts_with('/') {
        return url.to_string();
    }
    // 区切りは `/`。Windows のパス（`D:\…\catalog.json`、`file://D:\…`）では `\` も区切り。
    // ただし `file://` や `https://` の `//` は区切りとみなさない
    let start = catalog_url.find("://").map_or(0, |i| i + 3);
    match catalog_url[start..].rfind(['/', '\\']) {
        Some(i) => {
            let at = start + i;
            let sep = &catalog_url[at..at + 1];
            format!("{}{sep}{}", &catalog_url[..at], url.replace('/', sep))
        }
        None => url.to_string(),
    }
}

fn safe_relative(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && p.split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.starts_with('.'))
}

/// プラグインを入れる（同じ id のものがあれば置き換える）。入れたフォルダを返す
pub fn install(root: &Path, entry: &Entry, catalog_url: &str) -> Result<PathBuf> {
    if entry.id.is_empty()
        || !entry
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("id「{}」には英数字・-・_ だけが使えます", entry.id);
    }
    if !entry.files.contains_key("plugin.toml") {
        bail!("plugin.toml がありません");
    }
    // 先にすべて取ってきて確かめる（途中で失敗しても、今のプラグインは壊さない）
    let mut files = Vec::new();
    for (path, f) in &entry.files {
        if !safe_relative(path) {
            bail!("プラグインのフォルダの外を指すファイルは入れられません: {path}");
        }
        let url = resolve(catalog_url, &f.url);
        let bytes = fetch(&url)?;
        let hash = plugin::hex(&sha2::Sha256::digest(&bytes));
        if !hash.eq_ignore_ascii_case(f.sha256.trim()) {
            bail!(
                "{path} の sha256 が一覧と違います（一覧: {}、取得したもの: {hash}）",
                f.sha256
            );
        }
        files.push((path.clone(), bytes));
    }
    let dir = plugin::dir(root);
    let tmp = dir.join(format!(".install-{}", entry.id));
    let _ = std::fs::remove_dir_all(&tmp);
    for (path, bytes) in &files {
        let to = tmp.join(path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&to, bytes)?;
    }
    // plugin.toml が読めることを確かめる
    let text = std::fs::read_to_string(tmp.join("plugin.toml"))?;
    if let Err(e) = plugin::parse_manifest(&text) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e.context("plugin.toml の書き方が正しくありません"));
    }
    let target = dir.join(&entry.id);
    let old = dir.join(format!(".old-{}", entry.id));
    let _ = std::fs::remove_dir_all(&old);
    if target.exists() {
        std::fs::rename(&target, &old).context("今のプラグインを退かせません")?;
    }
    if let Err(e) = std::fs::rename(&tmp, &target) {
        // 戻す
        let _ = std::fs::rename(&old, &target);
        return Err(anyhow!(e).context("プラグインを置けません"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sha(b: &[u8]) -> String {
        plugin::hex(&sha2::Sha256::digest(b))
    }

    #[test]
    fn installs_from_a_local_catalog() {
        let src = tempfile::tempdir().unwrap();
        let toml = b"name = \"hello\"\nmode = \"wasm\"\npermissions = [\"read\"]\n";
        let wasm = wat::parse_str(
            "(module (memory (export \"memory\") 1) (func (export \"alloc\") (param i32) (result i32) i32.const 0) (func (export \"handle\") (param i32 i32) (result i64) i64.const 0))",
        )
        .unwrap();
        fs::create_dir_all(src.path().join("hello")).unwrap();
        fs::write(src.path().join("hello/plugin.toml"), toml).unwrap();
        fs::write(src.path().join("hello/plugin.wasm"), &wasm).unwrap();
        let catalog = serde_json::json!({
            "plugins": [{
                "id": "hello",
                "name": "こんにちは",
                "description": "試し",
                "files": {
                    "plugin.toml": {"url": "hello/plugin.toml", "sha256": sha(toml)},
                    "plugin.wasm": {"url": "hello/plugin.wasm", "sha256": sha(&wasm)}
                }
            }]
        });
        let catalog_path = src.path().join("catalog.json");
        fs::write(&catalog_path, catalog.to_string()).unwrap();
        let url = format!("file://{}", catalog_path.display());

        let c = load(&url).unwrap();
        assert_eq!(search(&c, "こんにちは").len(), 1);
        assert_eq!(search(&c, "hello 試し").len(), 1);
        assert!(search(&c, "nope").is_empty());

        let vault = tempfile::tempdir().unwrap();
        let dir = install(vault.path(), &c.plugins[0], &url).unwrap();
        assert!(dir.join("plugin.wasm").exists());
        let (ok, bad) = plugin::discover(vault.path());
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(ok[0].manifest.mode, plugin::Mode::Wasm);
        // 許可の記録に .wasm のハッシュが入る
        assert!(ok[0].text.contains(&sha(&wasm)));

        // sha256 が違えば入れない（今のものは壊さない）
        let mut bad_entry = c.plugins[0].clone();
        bad_entry.files.get_mut("plugin.wasm").unwrap().sha256 = "00".into();
        assert!(install(vault.path(), &bad_entry, &url).is_err());
        assert!(dir.join("plugin.wasm").exists());

        // フォルダの外を指すファイルは入れない
        let mut escape = c.plugins[0].clone();
        let f = escape.files.remove("plugin.wasm").unwrap();
        escape.files.insert("../evil.md".into(), f);
        assert!(install(vault.path(), &escape, &url).is_err());
    }

    #[test]
    fn resolves_relative_urls() {
        assert_eq!(
            resolve("https://example.com/p/catalog.json", "a/plugin.toml"),
            "https://example.com/p/a/plugin.toml"
        );
        assert_eq!(resolve("https://x/c.json", "https://y/z"), "https://y/z");
        // Windows のパス
        assert_eq!(
            resolve("file://D:\\a\\catalog.json", "p/plugin.toml"),
            "file://D:\\a\\p\\plugin.toml"
        );
        assert_eq!(resolve("D:\\a\\catalog.json", "p/x"), "D:\\a\\p\\x");
        assert_eq!(resolve("file:///tmp/c.json", "p/x"), "file:///tmp/p/x");
        assert!(!safe_relative(".hidden/x"));
        assert!(safe_relative("lib/robsidian.py"));
    }
}
