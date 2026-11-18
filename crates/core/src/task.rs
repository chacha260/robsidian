//! タスク（`- [ ] ...`）の抽出と切り替え。
//!
//! 期日は Obsidian Tasks プラグインの `📅 2026-01-31` と、`due:2026-01-31`・Dataview の
//! `[due:: 2026-01-31]` を読む。Tasks プラグインの書き方で、ほかに次のものを読む。
//!
//! - 完了日 `✅ 2026-01-31`（完了にしたときに付け、未完了に戻したら外す）
//! - 優先度 `🔺`（最高）`⏫`（高）`🔼`（中）`🔽`（低）`⏬`（最低）
//! - 繰り返し `🔁 every week`（完了にすると、日付を進めた次のタスクを上の行に作る）

use chrono::{Datelike, Duration, NaiveDate, Weekday};
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
    /// 完了日（`✅ 日付`）
    pub done_date: Option<NaiveDate>,
    pub priority: Option<Priority>,
    /// 繰り返し（`🔁` の後ろの `every week` など）
    pub recurrence: Option<String>,
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

/// 優先度（Tasks プラグインの記号）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Priority {
    Highest,
    High,
    Medium,
    Low,
    Lowest,
}

impl Priority {
    pub const ALL: [Priority; 5] = [
        Priority::Highest,
        Priority::High,
        Priority::Medium,
        Priority::Low,
        Priority::Lowest,
    ];

    pub fn symbol(self) -> &'static str {
        match self {
            Priority::Highest => "🔺",
            Priority::High => "⏫",
            Priority::Medium => "🔼",
            Priority::Low => "🔽",
            Priority::Lowest => "⏬",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Priority::Highest => "最高",
            Priority::High => "高",
            Priority::Medium => "中",
            Priority::Low => "低",
            Priority::Lowest => "最低",
        }
    }

    /// 並べ替えの順（高いほど小さい。指定なしは「中」と「低」の間、Tasks プラグインと同じ）
    pub fn rank(p: Option<Priority>) -> u8 {
        match p {
            Some(Priority::Highest) => 0,
            Some(Priority::High) => 1,
            Some(Priority::Medium) => 2,
            None => 3,
            Some(Priority::Low) => 4,
            Some(Priority::Lowest) => 5,
        }
    }
}

/// 本文から読み取った、期日などの書き込み
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meta {
    /// 書き込みを除いた本文
    pub text: String,
    pub due: Option<NaiveDate>,
    pub done: Option<NaiveDate>,
    pub priority: Option<Priority>,
    pub recurrence: Option<String>,
}

/// Tasks プラグインの書き込みの記号（繰り返しの文がどこで終わるかを知るため）
const SIGNIFIERS: [&str; 11] = [
    "📅", "✅", "⏳", "🛫", "➕", "🔁", "🔺", "⏫", "🔼", "🔽", "⏬",
];

/// 本文から期日・完了日・優先度・繰り返しを取り出す
pub fn meta(body: &str) -> Meta {
    let (mut text, due) = split_due(body);
    let mut m = Meta {
        due,
        ..Meta::default()
    };
    // 完了日
    if let Some(i) = text.find("✅") {
        let after = text[i + "✅".len()..].trim_start();
        if let Some(d) = parse_date(after) {
            let end = text.len() - after.len() + 10;
            m.done = Some(d);
            text = format!("{}{}", &text[..i], &text[end..]);
        }
    }
    // 優先度（最初に見つかったもの）
    for p in Priority::ALL {
        if let Some(i) = text.find(p.symbol()) {
            m.priority = Some(p);
            text.replace_range(i..i + p.symbol().len(), "");
            break;
        }
    }
    // 繰り返し（次の記号か行末まで）
    if let Some(i) = text.find("🔁") {
        let start = i + "🔁".len();
        let end = SIGNIFIERS
            .iter()
            .filter_map(|s| text[start..].find(s).map(|j| start + j))
            .min()
            .unwrap_or(text.len());
        let rule = text[start..end].trim().to_string();
        if !rule.is_empty() {
            m.recurrence = Some(rule);
        }
        text.replace_range(i..end, "");
    }
    m.text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    m
}

/// 繰り返しの規則（`every day` / `every 2 weeks` / `every month` / `every monday` など）で、
/// `from` の次の日付。`when done` が付いていれば今日から数える（呼ぶ側で `from` を今日にする）
pub fn next_date(rule: &str, from: NaiveDate) -> Option<NaiveDate> {
    let rule = rule.to_lowercase();
    let rule = rule.trim().trim_end_matches("when done").trim();
    let rest = rule.strip_prefix("every")?.trim();
    let mut words = rest.split_whitespace();
    let first = words.next().unwrap_or("day");
    let (n, unit) = match first.parse::<u32>() {
        Ok(n) => (n.max(1), words.next().unwrap_or("day")),
        Err(_) => (1, first),
    };
    let unit = unit.trim_end_matches('s');
    let weekday = |name: &str| -> Option<Weekday> {
        Some(match name {
            "monday" | "mon" => Weekday::Mon,
            "tuesday" | "tue" => Weekday::Tue,
            "wednesday" | "wed" => Weekday::Wed,
            "thursday" | "thu" => Weekday::Thu,
            "friday" | "fri" => Weekday::Fri,
            "saturday" | "sat" => Weekday::Sat,
            "sunday" | "sun" => Weekday::Sun,
            _ => return None,
        })
    };
    match unit {
        "day" => from.checked_add_signed(Duration::days(i64::from(n))),
        "week" => from.checked_add_signed(Duration::weeks(i64::from(n))),
        "month" => from.checked_add_months(chrono::Months::new(n)),
        "year" => from.checked_add_months(chrono::Months::new(n * 12)),
        "weekday" => {
            // 次の平日
            let mut d = from.succ_opt()?;
            while matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
                d = d.succ_opt()?;
            }
            Some(d)
        }
        name => {
            // 次のその曜日
            let want = weekday(name)?;
            let mut d = from.succ_opt()?;
            while d.weekday() != want {
                d = d.succ_opt()?;
            }
            Some(d)
        }
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

/// `n` 行目（0始まり）のタスクの完了状態を切り替えた本文（今日の日付で）。タスクでなければ None。
pub fn toggle(text: &str, n: usize) -> Option<String> {
    toggle_on(text, n, chrono::Local::now().date_naive())
}

/// `n` 行目のタスクの完了状態を切り替えた本文。Obsidian と同じく、未完了なら完了に、
/// それ以外なら未完了に戻す。完了にしたら `✅ today` を付け、繰り返しのタスクなら
/// 日付を進めた次のタスクを上の行に作る。未完了に戻したら `✅` を外す。
pub fn toggle_on(text: &str, n: usize, today: NaiveDate) -> Option<String> {
    let start = if n == 0 {
        0
    } else {
        text.match_indices('\n').nth(n - 1)?.0 + 1
    };
    let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
    let line = &text[start..end];
    let (at, status, body) = task_in_line(line)?;
    let body_start = line.len() - line[at..].len() + status.len_utf8() + 1;
    let line_end = line.trim_end();
    let mut out = text.to_string();
    if status == ' ' {
        // 完了にする: 状態の文字を x にして、行末に完了日
        let mut done = format!("{}x{}", &line[..at], &line_end[at + status.len_utf8()..]);
        done.push_str(&format!(" ✅ {}", today.format("%Y-%m-%d")));
        let mut replacement = done;
        let m = meta(body);
        if let Some(rule) = &m.recurrence {
            if let Some(next) = next_task(line, body_start, &m, rule, today) {
                replacement = format!("{next}\n{replacement}");
            }
        }
        out.replace_range(start..start + line_end.len(), &replacement);
    } else {
        // 未完了に戻す: 状態の文字を空白にして、完了日を外す
        let mut undone = format!("{} {}", &line[..at], &line_end[at + status.len_utf8()..]);
        if let Some(i) = undone.find("✅") {
            let after = undone[i + "✅".len()..].trim_start();
            if parse_date(after).is_some() {
                let end = undone.len() - after.len() + 10;
                let before = undone[..i].trim_end().len();
                undone.replace_range(before..end, "");
            }
        }
        out.replace_range(start..start + line_end.len(), &undone);
    }
    Some(out)
}

/// 繰り返しのタスクの、次の回の行（状態は未完了、期日などの日付を進める）
fn next_task(
    line: &str,
    body_start: usize,
    m: &Meta,
    rule: &str,
    today: NaiveDate,
) -> Option<String> {
    let when_done = rule.to_lowercase().contains("when done");
    // 期日（なければ今日）から次の日を出し、期日以外の日付（⏳ 🛫）も同じだけずらす
    let base = if when_done {
        today
    } else {
        m.due.unwrap_or(today)
    };
    let next_due = next_date(rule, base)?;
    let shift = next_due - base;
    let head = &line[..body_start];
    let head = match head.rfind('[') {
        Some(i) => format!("{}[ ]{}", &head[..i], &head[i + 3..]),
        None => head.to_string(),
    };
    let mut body = line[body_start..].trim_end().to_string();
    // 日付を書き換える（📅 ⏳ 🛫 と due: などの後ろの日付）
    for key in ["📅", "⏳", "🛫", "[due::", "due::", "due:"] {
        if let Some(i) = body.find(key) {
            let after_at = i + key.len();
            let after = body[after_at..].trim_start();
            let date_at = body.len() - after.len();
            if let Some(d) = parse_date(after) {
                let moved = if key == "📅" || key.contains("due") {
                    next_due
                } else {
                    d + shift
                };
                body.replace_range(date_at..date_at + 10, &moved.format("%Y-%m-%d").to_string());
            }
        }
    }
    let _ = when_done;
    Some(format!("{head}{body}"))
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
        let today = NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
        let t = "# T\n- [ ] a\n- [x] b\nc";
        assert_eq!(
            toggle_on(t, 1, today).unwrap(),
            "# T\n- [x] a ✅ 2026-01-10\n- [x] b\nc"
        );
        assert_eq!(toggle(t, 2).unwrap(), "# T\n- [ ] a\n- [ ] b\nc");
        assert!(toggle(t, 3).is_none());
        assert!(toggle(t, 9).is_none());
    }

    #[test]
    fn reads_priority_done_and_recurrence() {
        let m = meta("週報を書く ⏫ 🔁 every week 📅 2026-01-05 ✅ 2026-01-04");
        assert_eq!(m.text, "週報を書く");
        assert_eq!(m.priority, Some(Priority::High));
        assert_eq!(m.recurrence.as_deref(), Some("every week"));
        assert_eq!(m.due, NaiveDate::from_ymd_opt(2026, 1, 5));
        assert_eq!(m.done, NaiveDate::from_ymd_opt(2026, 1, 4));
        assert_eq!(meta("ただの文").text, "ただの文");
        assert!(Priority::rank(Some(Priority::High)) < Priority::rank(None));
        assert!(Priority::rank(None) < Priority::rank(Some(Priority::Low)));
    }

    #[test]
    fn next_dates() {
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        assert_eq!(next_date("every day", d(2026, 1, 31)), Some(d(2026, 2, 1)));
        assert_eq!(
            next_date("every 2 weeks", d(2026, 1, 1)),
            Some(d(2026, 1, 15))
        );
        assert_eq!(
            next_date("every month", d(2026, 1, 31)),
            Some(d(2026, 2, 28))
        );
        assert_eq!(next_date("every year", d(2026, 3, 1)), Some(d(2027, 3, 1)));
        // 2026-01-02 は金曜日
        assert_eq!(
            next_date("every weekday", d(2026, 1, 2)),
            Some(d(2026, 1, 5))
        );
        assert_eq!(
            next_date("every Monday", d(2026, 1, 2)),
            Some(d(2026, 1, 5))
        );
        assert_eq!(
            next_date("every week when done", d(2026, 1, 1)),
            Some(d(2026, 1, 8))
        );
        assert_eq!(next_date("sometimes", d(2026, 1, 1)), None);
    }

    #[test]
    fn toggling_adds_done_date_and_next_occurrence() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
        // 完了にすると完了日、戻すと外れる
        let t = "- [ ] a ⏫\nb";
        let done = toggle_on(t, 0, today).unwrap();
        assert_eq!(done, "- [x] a ⏫ ✅ 2026-01-10\nb");
        assert_eq!(toggle_on(&done, 0, today).unwrap(), t);
        // 繰り返し: 次の回を上の行に作る
        let t = "  - [ ] ゴミ出し 🔁 every week 📅 2026-01-07";
        assert_eq!(
            toggle_on(t, 0, today).unwrap(),
            "  - [ ] ゴミ出し 🔁 every week 📅 2026-01-14\n  - [x] ゴミ出し 🔁 every week 📅 2026-01-07 ✅ 2026-01-10"
        );
        // 完了した日から数える
        let t = "- [ ] 散髪 🔁 every 4 weeks when done 📅 2026-01-01";
        assert!(toggle_on(t, 0, today)
            .unwrap()
            .starts_with("- [ ] 散髪 🔁 every 4 weeks when done 📅 2026-02-07\n"));
    }
}
