//! タスク（`- [ ] ...`）の抽出と切り替え。
//!
//! 期日は Obsidian Tasks プラグインの `📅 2026-01-31` と、`due:2026-01-31`・Dataview の
//! `[due:: 2026-01-31]` を読む。

use chrono::NaiveDate;
use serde::Serialize;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task {
    /// 0始まりの行番号
    pub line: usize,
    /// `[ ]` の中の文字（' ' は未完了、'x' は完了、'-' は取り消し など）
    pub status: char,
    /// 記号と期日を除いた本文
    pub text: String,
    pub due: Option<NaiveDate>,
    /// 本文中で状態の文字が書かれているバイト範囲
    #[serde(skip)]
    pub marker: Range<usize>,
}

impl Task {
    /// 完了（または取り消し）か。
    pub fn done(&self) -> bool {
        matches!(self.status, 'x' | 'X' | '-')
    }
}

/// 行がタスクなら、(状態の文字の行内バイト位置, 状態の文字, その後ろの本文)。
pub fn task_in_line(line: &str) -> Option<(usize, char, &str)> {
    let mut pos = line.len() - line.trim_start_matches([' ', '\t']).len();
    while line[pos..].starts_with('>') {
        pos += 1;
        pos += line[pos..].len() - line[pos..].trim_start_matches(' ').len();
    }
    let rest = &line[pos..];
    let marker_len = if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        2
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let ok = (1..10).contains(&digits)
            && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
            && rest.as_bytes().get(digits + 1) == Some(&b' ');
        if !ok {
            return None;
        }
        digits + 2
    };
    let after = &rest[marker_len..];
    let mut chars = after.char_indices();
    if chars.next() != Some((0, '[')) {
        return None;
    }
    let (_, status) = chars.next()?;
    let close = 1 + status.len_utf8();
    if after.as_bytes().get(close) != Some(&b']') {
        return None;
    }
    let body = &after[close + 1..];
    if !(body.is_empty() || body.starts_with(' ')) {
        return None;
    }
    Some((pos + marker_len + 1, status, body.trim()))
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

/// 本文から期日を取り出し、期日の書き込みを除いた本文と一緒に返す。
pub fn split_due(body: &str) -> (String, Option<NaiveDate>) {
    for key in ["📅", "[due::", "due::", "due:"] {
        if let Some(i) = body.find(key) {
            let after = body[i + key.len()..].trim_start();
            if let Some(date) = parse_date(after) {
                let date_end = body.len() - after.len() + 10;
                let end = if key == "[due::" {
                    body[date_end..]
                        .find(']')
                        .map_or(date_end, |j| date_end + j + 1)
                } else {
                    date_end
                };
                let text = format!("{}{}", &body[..i], &body[end..]);
                return (
                    text.split_whitespace().collect::<Vec<_>>().join(" "),
                    Some(date),
                );
            }
        }
    }
    (body.to_string(), None)
}

/// `n` 行目（0始まり）のタスクの完了状態を切り替えた本文。タスクでなければ None。
pub fn toggle(text: &str, n: usize) -> Option<String> {
    let start = if n == 0 {
        0
    } else {
        text.match_indices('\n').nth(n - 1)?.0 + 1
    };
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let (at, status, _) = task_in_line(&text[start..end])?;
    let at = start + at;
    let next = if status == ' ' { "x" } else { " " };
    let mut out = text.to_string();
    out.replace_range(at..at + status.len_utf8(), next);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_tasks() {
        assert_eq!(task_in_line("- [ ] buy milk"), Some((3, ' ', "buy milk")));
        assert_eq!(task_in_line("  * [x] done"), Some((5, 'x', "done")));
        assert_eq!(task_in_line("> - [/] doing"), Some((5, '/', "doing")));
        assert_eq!(task_in_line("1. [ ] first"), Some((4, ' ', "first")));
        assert_eq!(task_in_line("- [ ]"), Some((3, ' ', "")));
        assert_eq!(task_in_line("- [あ] 全角"), Some((3, 'あ', "全角")));
        assert!(task_in_line("- [link](x)").is_none());
        assert!(task_in_line("- [ ]x").is_none());
        assert!(task_in_line("[ ] no marker").is_none());
        assert!(task_in_line("plain").is_none());
    }

    #[test]
    fn due_dates() {
        let d = |s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
        assert_eq!(
            split_due("pay 📅 2026-01-31 now"),
            ("pay now".into(), d("2026-01-31"))
        );
        assert_eq!(
            split_due("pay due:2026-02-01"),
            ("pay".into(), d("2026-02-01"))
        );
        assert_eq!(
            split_due("pay [due:: 2026-03-01] x"),
            ("pay x".into(), d("2026-03-01"))
        );
        assert_eq!(split_due("no date"), ("no date".into(), None));
        assert_eq!(split_due("due:soon"), ("due:soon".into(), None));
    }

    #[test]
    fn toggling() {
        let t = "# T\n- [ ] a\n- [x] b\nc";
        assert_eq!(toggle(t, 1).unwrap(), "# T\n- [x] a\n- [x] b\nc");
        assert_eq!(toggle(t, 2).unwrap(), "# T\n- [ ] a\n- [ ] b\nc");
        assert!(toggle(t, 3).is_none());
        assert!(toggle(t, 9).is_none());
    }
}
