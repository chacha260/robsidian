//! テンプレートの展開と、Obsidian（moment.js）形式の日付書式。
//!
//! 対応する変数: `{{title}}`、`{{date}}`、`{{time}}`、`{{date:書式}}`、`{{time:書式}}`。
//! Obsidianのコアプラグイン「テンプレート」と同じ書き方。

use chrono::{DateTime, Datelike, Local, NaiveDateTime, Timelike};

/// moment.js 形式の書式で日時を文字列にする。
///
/// 対応: `YYYY YY MM M DD D HH H hh h mm m ss s A a dddd ddd`。`[...]` の中はそのまま出す。
/// それ以外の文字もそのまま出す。
pub fn format_moment(t: &NaiveDateTime, format: &str) -> String {
    const WEEKDAYS: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    let mut out = String::with_capacity(format.len() + 8);
    let mut rest = format;
    while let Some(c) = rest.chars().next() {
        if c == '[' {
            if let Some(end) = rest.find(']') {
                out.push_str(&rest[1..end]);
                rest = &rest[end + 1..];
                continue;
            }
        }
        let run = rest.chars().take_while(|&x| x == c).count();
        let token = &rest[..run * c.len_utf8()];
        let hour12 = match t.hour() % 12 {
            0 => 12,
            h => h,
        };
        let weekday = WEEKDAYS[t.weekday().num_days_from_monday() as usize];
        let (text, used) = match (c, run) {
            ('Y', n) if n >= 4 => (format!("{:04}", t.year()), 4),
            ('Y', 2 | 3) => (format!("{:02}", t.year() % 100), 2),
            ('M', n) if n >= 2 => (format!("{:02}", t.month()), 2),
            ('M', 1) => (t.month().to_string(), 1),
            ('D', n) if n >= 2 => (format!("{:02}", t.day()), 2),
            ('D', 1) => (t.day().to_string(), 1),
            ('H', n) if n >= 2 => (format!("{:02}", t.hour()), 2),
            ('H', 1) => (t.hour().to_string(), 1),
            ('h', n) if n >= 2 => (format!("{hour12:02}"), 2),
            ('h', 1) => (hour12.to_string(), 1),
            ('m', n) if n >= 2 => (format!("{:02}", t.minute()), 2),
            ('m', 1) => (t.minute().to_string(), 1),
            ('s', n) if n >= 2 => (format!("{:02}", t.second()), 2),
            ('s', 1) => (t.second().to_string(), 1),
            ('A', _) => ((if t.hour() < 12 { "AM" } else { "PM" }).to_string(), 1),
            ('a', _) => ((if t.hour() < 12 { "am" } else { "pm" }).to_string(), 1),
            ('d', n) if n >= 4 => (weekday.to_string(), 4),
            ('d', 3) => (weekday[..3].to_string(), 3),
            _ => (token.to_string(), run),
        };
        out.push_str(&text);
        rest = &rest[used * c.len_utf8()..];
    }
    out
}

/// テンプレートを展開する。
pub fn render(template: &str, title: &str, now: &DateTime<Local>) -> String {
    let now = now.naive_local();
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inner = after[..close].trim();
        let (name, format) = match inner.split_once(':') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (inner, None),
        };
        let value = match (name.to_lowercase().as_str(), format) {
            ("title", _) => Some(title.to_string()),
            ("date", f) => Some(format_moment(&now, f.unwrap_or("YYYY-MM-DD"))),
            ("time", f) => Some(format_moment(&now, f.unwrap_or("HH:mm"))),
            _ => None,
        };
        match value {
            Some(v) => out.push_str(&v),
            // 知らない変数は手を付けない（他のプラグイン向けの書き方かもしれない）
            None => out.push_str(&rest[open..open + 2 + close + 2]),
        }
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    fn at() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 3, 7)
            .unwrap()
            .and_hms_opt(9, 5, 3)
            .unwrap()
    }

    #[test]
    fn moment_tokens() {
        let t = at();
        assert_eq!(format_moment(&t, "YYYY-MM-DD"), "2026-03-07");
        assert_eq!(format_moment(&t, "YYYY/M/D HH:mm:ss"), "2026/3/7 09:05:03");
        assert_eq!(format_moment(&t, "YY年M月D日(ddd)"), "26年3月7日(Sat)");
        assert_eq!(format_moment(&t, "dddd h:mm A"), "Saturday 9:05 AM");
        assert_eq!(format_moment(&t, "[Week of] YYYY"), "Week of 2026");
    }

    #[test]
    fn render_variables() {
        let now = Local.from_local_datetime(&at()).unwrap();
        let tpl = "# {{title}}\n作成: {{date}} {{time}}\n{{date:YYYY年M月D日}} {{ time : HH }} {{unknown}} {{";
        assert_eq!(
            render(tpl, "会議", &now),
            "# 会議\n作成: 2026-03-07 09:05\n2026年3月7日 09 {{unknown}} {{"
        );
    }
}
