//! テーマ設定ファイル（`.robsidian/theme.toml`）: 配色をダーク/ライトそれぞれで上書きする。
//!
//! ```toml
//! [dark]
//! background = "#1e1f24"
//! accent = "#7aa2f7"
//!
//! [light]
//! accent = "#2a5bd7"
//! ```
//!
//! 色は `#rgb` / `#rrggbb` / `#rrggbbaa`。どのキーが使えるかはGUI側が決める（ここでは名前と色の対応だけを読む）。

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// RGBA
pub type Rgba = [u8; 4];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Theme {
    pub dark: BTreeMap<String, Rgba>,
    pub light: BTreeMap<String, Rgba>,
}

impl Theme {
    pub fn colors(&self, dark: bool) -> &BTreeMap<String, Rgba> {
        if dark {
            &self.dark
        } else {
            &self.light
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    dark: BTreeMap<String, String>,
    #[serde(default)]
    light: BTreeMap<String, String>,
}

pub fn path(root: &Path) -> PathBuf {
    root.join(".robsidian").join("theme.toml")
}

/// `#rgb` / `#rrggbb` / `#rrggbbaa`
pub fn parse_color(s: &str) -> Result<Rgba> {
    let hex = s
        .trim()
        .strip_prefix('#')
        .ok_or_else(|| anyhow!("色は # で始めてください: {s}"))?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("色の書き方が正しくありません: {s}");
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
    Ok(match hex.len() {
        3 => {
            let n = |i: usize| u8::from_str_radix(&hex[i..=i], 16).unwrap_or(0) * 17;
            [n(0), n(1), n(2), 255]
        }
        6 => [byte(0), byte(2), byte(4), 255],
        8 => [byte(0), byte(2), byte(4), byte(6)],
        _ => bail!("色の書き方が正しくありません: {s}"),
    })
}

pub fn parse(text: &str) -> Result<Theme> {
    let file: File = toml::from_str(text)?;
    let convert = |table: BTreeMap<String, String>, section: &str| -> Result<_> {
        table
            .into_iter()
            .map(|(k, v)| {
                let c = parse_color(&v).with_context(|| format!("[{section}] の {k}"))?;
                Ok((k, c))
            })
            .collect::<Result<BTreeMap<_, _>>>()
    };
    Ok(Theme {
        dark: convert(file.dark, "dark")?,
        light: convert(file.light, "light")?,
    })
}

/// ファイルが無ければ `None`。
pub fn load(root: &Path) -> Result<Option<Theme>> {
    let p = path(root);
    match std::fs::read_to_string(&p) {
        Ok(text) => parse(&text)
            .with_context(|| format!("{} の書き方が正しくありません", p.display()))
            .map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", p.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors() {
        assert_eq!(parse_color("#fff").unwrap(), [255, 255, 255, 255]);
        assert_eq!(parse_color("#1a2B3c").unwrap(), [0x1a, 0x2b, 0x3c, 255]);
        assert_eq!(parse_color(" #00000080 ").unwrap(), [0, 0, 0, 0x80]);
        assert!(parse_color("fff").is_err());
        assert!(parse_color("#ffff").is_err());
        assert!(parse_color("#ggg").is_err());
        assert!(parse_color("#ｆｆｆ").is_err());
    }

    #[test]
    fn parses_file() {
        let t = parse("[dark]\naccent = \"#ff0000\"\n[light]\ntext = \"#000\"\n").unwrap();
        assert_eq!(t.colors(true)["accent"], [255, 0, 0, 255]);
        assert_eq!(t.colors(false)["text"], [0, 0, 0, 255]);
        assert!(parse("").unwrap().dark.is_empty());
        let err = parse("[dark]\naccent = \"red\"\n").unwrap_err();
        assert!(format!("{err:#}").contains("accent"), "{err:#}");
        assert!(parse("[dakr]\n").is_err());
    }
}
