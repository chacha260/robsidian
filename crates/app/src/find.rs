//! ノート内の検索と置換。

use regex::{Regex, RegexBuilder};
use std::ops::Range;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Query {
    pub text: String,
    /// 大文字小文字を区別する
    pub case: bool,
    /// 正規表現として扱う（置換文字列では `$1` などが使える）
    pub regex: bool,
}

impl Query {
    fn compile(&self) -> Result<Option<Regex>, String> {
        if self.text.is_empty() {
            return Ok(None);
        }
        let pattern = if self.regex {
            self.text.clone()
        } else {
            regex::escape(&self.text)
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.case)
            .multi_line(true)
            .build()
            .map(Some)
            .map_err(|e| e.to_string())
    }

    /// すべての一致（空文字に一致するものは除く）。正規表現の誤りはErr。
    pub fn find_all(&self, text: &str) -> Result<Vec<Range<usize>>, String> {
        let Some(re) = self.compile()? else {
            return Ok(Vec::new());
        };
        Ok(re
            .find_iter(text)
            .filter(|m| !m.is_empty())
            .map(|m| m.range())
            .collect())
    }

    /// `range` の一致を置き換えた文字列（正規表現なら `$1` などを展開する）。
    pub fn replacement_for(&self, text: &str, range: Range<usize>, with: &str) -> String {
        match self.compile() {
            Ok(Some(re)) if self.regex => {
                let hay = &text[range.clone()];
                match re.captures(hay) {
                    Some(caps) => {
                        let mut out = String::new();
                        caps.expand(with, &mut out);
                        out
                    }
                    None => with.to_string(),
                }
            }
            _ => with.to_string(),
        }
    }

    /// すべて置き換えた本文と、置き換えた件数。
    pub fn replace_all(&self, text: &str, with: &str) -> Result<(String, usize), String> {
        let matches = self.find_all(text)?;
        let mut out = text.to_string();
        for range in matches.iter().rev() {
            let rep = self.replacement_for(text, range.clone(), with);
            out.replace_range(range.clone(), &rep);
        }
        Ok((out, matches.len()))
    }
}

/// `cursor` 以降で最初の一致（無ければ先頭に戻る）の番号。
pub fn next_from(matches: &[Range<usize>], cursor: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(matches.iter().position(|m| m.start >= cursor).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str, case: bool, regex: bool) -> Query {
        Query {
            text: text.into(),
            case,
            regex,
        }
    }

    #[test]
    fn finding() {
        let text = "Rust rust RUST 日本語 rust.";
        assert_eq!(q("rust", false, false).find_all(text).unwrap().len(), 4);
        assert_eq!(
            q("rust", true, false).find_all(text).unwrap(),
            [5..9, 25..29]
        );
        assert_eq!(
            q("日本", false, false).find_all(text).unwrap(),
            vec![15..21]
        );
        // 正規表現の記号は、正規表現モードでなければ文字として探す
        assert_eq!(
            q("rust.", false, false).find_all(text).unwrap(),
            vec![25..30]
        );
        assert_eq!(
            q(r"r\w+", true, true).find_all(text).unwrap(),
            [5..9, 25..29]
        );
        assert!(q("(", false, true).find_all(text).is_err());
        assert!(q("", false, false).find_all(text).unwrap().is_empty());
        // 空文字に一致するパターンは数えない
        assert!(q("x*", false, true).find_all("abc").unwrap().is_empty());
        assert_eq!(q("^- ", false, true).find_all("- a\n- b").unwrap().len(), 2);
    }

    #[test]
    fn replacing() {
        let text = "Rust rust";
        let (out, n) = q("rust", false, false).replace_all(text, "Go").unwrap();
        assert_eq!((out.as_str(), n), ("Go Go", 2));
        let re = q(r"(\w)(\w+)", true, true);
        assert_eq!(re.replace_all("ab cd", "$2$1").unwrap().0, "ba dc");
        assert_eq!(re.replacement_for("ab cd", 3..5, "<$1>"), "<c>");
        // 通常モードでは $ はそのまま
        assert_eq!(q("a", false, false).replace_all("a", "$1").unwrap().0, "$1");
        assert_eq!(next_from(&[2..3, 8..9], 5), Some(1));
        assert_eq!(next_from(&[2..3, 8..9], 9), Some(0));
        assert_eq!(next_from(&[], 0), None);
    }
}
