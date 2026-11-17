//! コードブロックのシンタックスハイライト（syntect）。
//!
//! カーソルが動くたびにエディタ全体を組み直すので、色分け結果はブロック単位でキャッシュする。

use eframe::egui::Color32;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// 行ごとの (色, バイト数) の並び。各行のバイト数の合計は、改行を含むその行の長さ以上になる。
pub type Lines = Rc<Vec<Vec<(Color32, usize)>>>;

static ASSETS: LazyLock<(SyntaxSet, ThemeSet)> = LazyLock::new(|| {
    (
        SyntaxSet::load_defaults_newlines(),
        ThemeSet::load_defaults(),
    )
});

const MAX_CACHED_BLOCKS: usize = 200;

thread_local! {
    static CACHE: RefCell<HashMap<u64, Lines>> = RefCell::new(HashMap::new());
}

/// `lang`（```の後ろの言語名や拡張子）でコードを色分けする。未対応の言語は None。
/// `lines` は改行を含む各行。
pub fn code(lang: &str, lines: &[&str], dark: bool) -> Option<Lines> {
    if lang.is_empty() || lines.is_empty() {
        return None;
    }
    let (syntaxes, themes) = &*ASSETS;
    let syntax = syntaxes.find_syntax_by_token(lang)?;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (lang, dark, lines).hash(&mut hasher);
    let key = hasher.finish();
    if let Some(hit) = CACHE.with_borrow(|c| c.get(&key).cloned()) {
        return Some(hit);
    }

    let theme = themes.themes.get(if dark {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    })?;
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        // 文法定義は行末の改行を前提にしているので、最終行に無ければ補う
        let owned;
        let line = if line.ends_with('\n') {
            line
        } else {
            owned = format!("{line}\n");
            owned.as_str()
        };
        let regions = highlighter.highlight_line(line, syntaxes).ok()?;
        out.push(
            regions
                .into_iter()
                .map(|(style, piece)| {
                    let c = style.foreground;
                    (Color32::from_rgb(c.r, c.g, c.b), piece.len())
                })
                .collect(),
        );
    }
    let out = Rc::new(out);
    CACHE.with_borrow_mut(|cache| {
        if cache.len() >= MAX_CACHED_BLOCKS {
            cache.clear();
        }
        cache.insert(key, out.clone());
    });
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_cover_each_line() {
        let lines = ["fn main() {\n", "    let s = \"x\"; // c\n", "}"];
        let spans = code("rust", &lines, true).expect("rust is a bundled syntax");
        assert_eq!(spans.len(), 3);
        for (line, spans) in lines.iter().zip(spans.iter()) {
            let total: usize = spans.iter().map(|(_, len)| len).sum();
            assert!(total >= line.len());
        }
        // キーワードと文字列が別の色になっている
        let distinct: std::collections::HashSet<_> = spans[1].iter().map(|(c, _)| *c).collect();
        assert!(distinct.len() >= 3);
        // 拡張子でも引ける。知らない言語は色分けしない
        assert!(code("py", &["x = 1\n"], false).is_some());
        assert!(code("no-such-lang", &["x\n"], true).is_none());
        assert!(code("", &["x\n"], true).is_none());
    }
}
