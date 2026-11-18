//! frontmatter（ノート先頭のYAML）のプロパティの読み書き。
//!
//! YAMLを完全には解釈せず、Obsidianのプロパティで使われる形（`キー: 値`、`キー: [a, b]`、
//! `キー:` に続く `  - 項目` のリスト）だけを扱う。それ以外（入れ子のマップや複数行の文字列など）は
//! 元の行のまま `Raw` として保ち、書き換えない。変更は該当するキーの行だけを置き換えるので、
//! コメントや他の項目の書き方は崩れない。

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
    /// 扱えない形。キーの行から次のキーの手前までの元の行
    Raw(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prop {
    pub key: String,
    pub value: Value,
    /// 本文中のバイト範囲（キーの行の先頭から、この項目の最後の行の改行の後ろまで）
    pub range: Range<usize>,
}

/// frontmatterの中身の範囲（`---` の行を除く）と、閉じ `---` の行の後ろの位置。
fn bounds(text: &str) -> Option<(Range<usize>, usize)> {
    let first = text.split_inclusive('\n').next()?;
    if first.trim_end() != "---" {
        return None;
    }
    let start = first.len();
    let mut pos = start;
    for raw in text[start..].split_inclusive('\n') {
        let t = raw.trim_end();
        if t == "---" || t == "..." {
            return Some((start..pos, pos + raw.len()));
        }
        pos += raw.len();
    }
    None
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        let inner = &s[1..s.len() - 1];
        if s.starts_with('"') {
            return inner.replace("\\\"", "\"").replace("\\\\", "\\");
        }
        return inner.replace("''", "'");
    }
    s.to_string()
}

/// 値をYAMLの1行に書ける形にする（必要なら引用符で囲む）。
fn quote(s: &str) -> String {
    let needs = s.is_empty()
        || s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || s.starts_with([
            '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`', '[', '{', '-', '?', ' ',
        ])
        || s.ends_with(' ')
        || matches!(s, "true" | "false" | "null" | "~" | "yes" | "no");
    if needs {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// プロパティの一覧。frontmatterが無ければ None。
pub fn read(text: &str) -> Option<Vec<Prop>> {
    let (inner, _) = bounds(text)?;
    let mut props: Vec<Prop> = Vec::new();
    let mut pos = inner.start;
    let lines: Vec<&str> = text[inner.clone()].split_inclusive('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let start = pos;
        pos += line.len();
        i += 1;
        let content = line.trim_end_matches(['\n', '\r']);
        if content.trim().is_empty()
            || content.trim_start().starts_with('#')
            || content.starts_with([' ', '\t', '-'])
        {
            // 空行・コメント・キーの無い行は、直前の項目に含めない（読み飛ばす）
            continue;
        }
        let Some((key, rest)) = content.split_once(':') else {
            continue;
        };
        let key = key.trim().to_string();
        let rest = rest.trim();
        // 続く字下げされた行（リストの項目や入れ子）を集める
        let mut children: Vec<&str> = Vec::new();
        while i < lines.len() {
            let next = lines[i].trim_end_matches(['\n', '\r']);
            if next.starts_with([' ', '\t', '-'])
                || next.trim().is_empty()
                    && i + 1 < lines.len()
                    && lines[i + 1].starts_with([' ', '\t', '-'])
            {
                children.push(next);
                pos += lines[i].len();
                i += 1;
            } else {
                break;
            }
        }
        let value = if rest.is_empty() && children.is_empty() {
            Value::Text(String::new())
        } else if rest.is_empty()
            && children
                .iter()
                .all(|c| c.trim().is_empty() || c.trim_start().starts_with("- ") || c.trim() == "-")
        {
            Value::List(
                children
                    .iter()
                    .filter(|c| !c.trim().is_empty())
                    .map(|c| unquote(c.trim_start().trim_start_matches('-')))
                    .filter(|v| !v.is_empty())
                    .collect(),
            )
        } else if children.is_empty() && rest.starts_with('[') && rest.ends_with(']') {
            Value::List(
                rest[1..rest.len() - 1]
                    .split(',')
                    .map(unquote)
                    .filter(|v| !v.is_empty())
                    .collect(),
            )
        } else if children.is_empty() && !rest.starts_with(['|', '>', '{', '&', '*']) {
            Value::Text(unquote(rest))
        } else {
            Value::Raw(text[start..pos].trim_end().to_string())
        };
        props.push(Prop {
            key,
            value,
            range: start..pos,
        });
    }
    Some(props)
}

fn render(key: &str, value: &Value) -> String {
    match value {
        Value::Text(t) => format!(
            "{key}: {}\n",
            if t.is_empty() {
                String::new()
            } else {
                quote(t)
            }
        )
        .replace(": \n", ":\n"),
        Value::List(items) if items.is_empty() => format!("{key}: []\n"),
        Value::List(items) => {
            let mut s = format!("{key}:\n");
            for item in items {
                s.push_str(&format!("  - {}\n", quote(item)));
            }
            s
        }
        Value::Raw(raw) => format!("{raw}\n"),
    }
}

/// プロパティを設定した本文。キーがあればその行だけを置き換え、無ければ末尾に足す。
/// frontmatterが無ければ作る。
pub fn set(text: &str, key: &str, value: &Value) -> String {
    let key = key.trim();
    let line = render(key, value);
    let Some((inner, _)) = bounds(text) else {
        return format!("---\n{line}---\n{text}");
    };
    let props = read(text).unwrap_or_default();
    let mut out = text.to_string();
    match props.iter().find(|p| p.key == key) {
        Some(p) => out.replace_range(p.range.clone(), &line),
        None => {
            // 閉じ `---` の直前に足す（最後の行に改行が無ければ補う）
            let at = inner.end;
            let sep = if at > inner.start && !text[..at].ends_with('\n') {
                "\n"
            } else {
                ""
            };
            out.insert_str(at, &format!("{sep}{line}"));
        }
    }
    out
}

/// プロパティを消した本文。
pub fn remove(text: &str, key: &str) -> String {
    let Some(p) = read(text).and_then(|ps| ps.into_iter().find(|p| p.key == key)) else {
        return text.to_string();
    };
    let mut out = text.to_string();
    out.replace_range(p.range, "");
    out
}

/// frontmatterの行（`---` を含む）の範囲。無ければ None。
pub fn block(text: &str) -> Option<Range<usize>> {
    bounds(text).map(|(_, end)| 0..end)
}

/// プロパティの型（Obsidian の `.obsidian/types.json` と同じ考え方）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropType {
    Text,
    List,
    Number,
    Checkbox,
    Date,
    DateTime,
}

impl PropType {
    pub const ALL: [PropType; 6] = [
        PropType::Text,
        PropType::List,
        PropType::Number,
        PropType::Checkbox,
        PropType::Date,
        PropType::DateTime,
    ];

    /// `types.json` の書き方から
    pub fn from_obsidian(name: &str) -> Option<Self> {
        Some(match name {
            "text" => PropType::Text,
            "multitext" | "tags" | "aliases" => PropType::List,
            "number" => PropType::Number,
            "checkbox" => PropType::Checkbox,
            "date" => PropType::Date,
            "datetime" => PropType::DateTime,
            _ => return None,
        })
    }

    /// `types.json` に書く名前
    pub fn obsidian_name(self) -> &'static str {
        match self {
            PropType::Text => "text",
            PropType::List => "multitext",
            PropType::Number => "number",
            PropType::Checkbox => "checkbox",
            PropType::Date => "date",
            PropType::DateTime => "datetime",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PropType::Text => "テキスト",
            PropType::List => "リスト",
            PropType::Number => "数値",
            PropType::Checkbox => "チェックボックス",
            PropType::Date => "日付",
            PropType::DateTime => "日付と時刻",
        }
    }

    /// 型の指定が無いとき、キーと値から推測する
    pub fn infer(key: &str, value: &Value) -> Self {
        if matches!(key, "tags" | "aliases" | "cssclasses") {
            return PropType::List;
        }
        let Value::Text(t) = value else {
            return match value {
                Value::List(_) => PropType::List,
                _ => PropType::Text,
            };
        };
        let t = t.trim();
        if matches!(t, "true" | "false") {
            PropType::Checkbox
        } else if !t.is_empty() && t.parse::<f64>().is_ok() {
            PropType::Number
        } else if parse_date(t).is_some() {
            PropType::Date
        } else if parse_datetime(t).is_some() {
            PropType::DateTime
        } else {
            PropType::Text
        }
    }
}

/// `2026-01-31`
pub fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

/// `2026-01-31T09:30`（秒と、空白区切りも受け付ける）
pub fn parse_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    let s = s.trim();
    [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ]
    .iter()
    .find_map(|f| chrono::NaiveDateTime::parse_from_str(s, f).ok())
}

/// 引用符を付けずに書く値（チェックボックスの `true` / `false` など）を設定した本文
pub fn set_plain(text: &str, key: &str, raw: &str) -> String {
    let key = key.trim();
    set(text, key, &Value::Raw(format!("{key}: {raw}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "---\ntitle: Hello: world\ntags: [a, \"b c\"]\naliases:\n  - Foo\n  - 'Bar'\n# comment\nnested:\n  k: v\nempty:\n---\n# Body\n";

    #[test]
    fn reading() {
        let props = read(DOC).unwrap();
        let got: Vec<(&str, &Value)> = props.iter().map(|p| (p.key.as_str(), &p.value)).collect();
        assert_eq!(
            got,
            [
                ("title", &Value::Text("Hello: world".into())),
                ("tags", &Value::List(vec!["a".into(), "b c".into()])),
                ("aliases", &Value::List(vec!["Foo".into(), "Bar".into()])),
                ("nested", &Value::Raw("nested:\n  k: v".into())),
                ("empty", &Value::Text(String::new())),
            ]
        );
        assert_eq!(
            &DOC[props[2].range.clone()],
            "aliases:\n  - Foo\n  - 'Bar'\n"
        );
        assert!(read("# no frontmatter").is_none());
        assert_eq!(block(DOC), Some(0..DOC.find("# Body").unwrap()));
    }

    #[test]
    fn writing_keeps_other_lines() {
        let out = set(DOC, "title", &Value::Text("New".into()));
        assert!(out.starts_with("---\ntitle: New\ntags: [a, \"b c\"]\n"));
        let out = set(DOC, "aliases", &Value::List(vec!["X".into()]));
        assert!(out.contains("aliases:\n  - X\n# comment\n"));
        let out = set(DOC, "status", &Value::Text("draft: v2".into()));
        assert!(out.contains("empty:\nstatus: \"draft: v2\"\n---\n# Body"));
        let out = remove(DOC, "tags");
        assert!(!out.contains("tags") && out.contains("title: Hello: world\naliases:"));
        // frontmatterが無ければ作る
        assert_eq!(
            set("body", "a", &Value::List(vec![])),
            "---\na: []\n---\nbody"
        );
        assert_eq!(
            set("---\n---\nx", "k", &Value::Text("v".into())),
            "---\nk: v\n---\nx"
        );
        // 書いたものを読み直すと同じ値になる
        for v in [
            Value::Text("#tag".into()),
            Value::Text("true".into()),
            Value::List(vec!["a: b".into()]),
        ] {
            let out = set(DOC, "x", &v);
            let back = read(&out)
                .unwrap()
                .into_iter()
                .find(|p| p.key == "x")
                .unwrap();
            assert_eq!(back.value, v);
        }
    }

    #[test]
    fn infers_types_and_writes_plain_values() {
        let t = |s: &str| Value::Text(s.into());
        assert_eq!(PropType::infer("done", &t("true")), PropType::Checkbox);
        assert_eq!(PropType::infer("pages", &t("120")), PropType::Number);
        assert_eq!(PropType::infer("due", &t("2026-01-31")), PropType::Date);
        assert_eq!(
            PropType::infer("at", &t("2026-01-31T09:30")),
            PropType::DateTime
        );
        assert_eq!(PropType::infer("title", &t("本")), PropType::Text);
        assert_eq!(PropType::infer("title", &t("")), PropType::Text);
        assert_eq!(PropType::infer("tags", &t("a")), PropType::List);
        assert_eq!(PropType::infer("x", &Value::List(vec![])), PropType::List);
        for ty in PropType::ALL {
            assert_eq!(PropType::from_obsidian(ty.obsidian_name()), Some(ty));
        }
        // チェックボックスは引用符なしで書き、読み直すと文字列の true になる
        let text = set_plain("---\ndone: false\n---\n本文", "done", "true");
        assert_eq!(text, "---\ndone: true\n---\n本文");
        assert_eq!(read(&text).unwrap()[0].value, t("true"));
    }
}
