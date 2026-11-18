//! 本文の数え上げと、脚注の一覧。

use serde::Serialize;

/// 文字数と単語数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Counts {
    /// 空白と改行を除いた文字数
    pub chars: usize,
    /// 単語数。欧文は空白や記号で区切った語、日本語などの文字（かな・漢字）は1文字を1語と数える
    /// （Obsidian の数え方に合わせる）
    pub words: usize,
}

/// 1文字を1語と数える文字（かな・漢字・ハングルなど）か
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30FF}'   // ひらがな・カタカナ
        | '\u{3400}'..='\u{4DBF}' // 漢字（拡張A）
        | '\u{4E00}'..='\u{9FFF}' // 漢字
        | '\u{F900}'..='\u{FAFF}' // 互換漢字
        | '\u{AC00}'..='\u{D7AF}' // ハングル
        | '\u{FF66}'..='\u{FF9F}' // 半角カタカナ
        | '\u{20000}'..='\u{2FA1F}')
}

pub fn count(text: &str) -> Counts {
    let mut counts = Counts::default();
    let mut in_word = false;
    for c in text.chars() {
        if !c.is_whitespace() {
            counts.chars += 1;
        }
        if is_cjk(c) {
            counts.words += 1;
            in_word = false;
        } else if c.is_alphanumeric() || c == '\'' || c == '_' {
            if !in_word {
                counts.words += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }
    counts
}

/// 脚注 `[^id]: 本文`
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Footnote {
    pub id: String,
    /// 定義の行（0始まり）。定義が無い参照だけの脚注は None
    pub line: Option<usize>,
    pub text: String,
    /// 本文中で参照している行（0始まり）
    pub refs: Vec<usize>,
}

/// 脚注の一覧（最初に参照された順。参照されていない定義は後ろ）。
/// コードブロックの中は数えない。
pub fn footnotes(text: &str) -> Vec<Footnote> {
    let mut list: Vec<Footnote> = Vec::new();
    let mut fence = false;
    let entry = |list: &mut Vec<Footnote>, id: &str| -> usize {
        match list.iter().position(|f| f.id == id) {
            Some(i) => i,
            None => {
                list.push(Footnote {
                    id: id.to_string(),
                    line: None,
                    text: String::new(),
                    refs: Vec::new(),
                });
                list.len() - 1
            }
        }
    };
    let mut defs = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        // 定義: 行頭の `[^id]:`
        let mut rest = line;
        if let Some(def) = t.strip_prefix("[^") {
            if let Some(end) = def.find("]:") {
                let id = &def[..end];
                if !id.is_empty() && !id.contains(char::is_whitespace) {
                    defs.push((id.to_string(), n, def[end + 2..].trim().to_string()));
                    rest = &def[end + 2..];
                }
            }
        }
        // 参照: `[^id]`（`:` が続かないもの）
        let mut s = rest;
        while let Some(start) = s.find("[^") {
            let after = &s[start + 2..];
            let Some(end) = after.find(']') else { break };
            let id = &after[..end];
            let is_def = after[end + 1..].starts_with(':');
            if !id.is_empty() && !id.contains(char::is_whitespace) && !is_def {
                let i = entry(&mut list, id);
                list[i].refs.push(n);
            }
            s = &after[end + 1..];
        }
    }
    for (id, line, body) in defs {
        let i = entry(&mut list, &id);
        list[i].line.get_or_insert(line);
        if list[i].text.is_empty() {
            list[i].text = body;
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_words_like_obsidian() {
        assert_eq!(
            count("Hello, world!"),
            Counts {
                chars: 12,
                words: 2
            }
        );
        assert_eq!(count("日本語のテキスト"), Counts { chars: 8, words: 8 });
        assert_eq!(
            count("Rust で書く app"),
            Counts {
                chars: 10,
                words: 5
            }
        );
        assert_eq!(
            count("don't stop_me 42"),
            Counts {
                chars: 14,
                words: 3
            }
        );
        assert_eq!(count("  \n\t "), Counts::default());
    }

    #[test]
    fn lists_footnotes() {
        let text = "本文[^a]と[^b]、また[^a]。\n```\n[^x] はコード\n```\n[^b]: Bの説明\n[^a]: Aの説明 [^c]\n[^unused]: 使われない";
        let f = footnotes(text);
        let ids: Vec<&str> = f.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c", "unused"]);
        assert_eq!(f[0].refs, [0, 0]);
        assert_eq!(f[0].line, Some(5));
        assert_eq!(f[0].text, "Aの説明 [^c]");
        assert_eq!(f[1].line, Some(4));
        // 定義の中から参照された脚注で、定義が無いもの
        assert_eq!(f[2].refs, [5]);
        assert_eq!(f[2].line, None);
        assert!(f[3].refs.is_empty());
        assert_eq!(f[3].line, Some(6));
    }
}
