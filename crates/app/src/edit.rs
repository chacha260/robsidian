//! エディタの編集補助（リストの継続、インデント、チェックボックス、強調、括弧の補完など）。
//!
//! どれも「本文と選択範囲（バイト位置）を受け取り、置き換えを返す」純粋関数にしてあり、
//! GUIなしでテストできる。

use std::ops::Range;

/// 本文の `range` を `insert` で置き換え、置き換え後の本文で `select` を選択状態にする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub insert: String,
    pub select: Range<usize>,
}

impl Edit {
    pub fn apply(&self, text: &mut String) {
        text.replace_range(self.range.clone(), &self.insert);
    }
}

fn line_start(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn line_end(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i)
}

/// 行頭の記号の構造。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Prefix {
    indent: String,
    /// `> ` の繰り返し（引用）
    quote: String,
    marker: Marker,
    /// 記号まで含めた長さ（バイト）
    len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Marker {
    None,
    Bullet(char),
    Task(char, bool),
    Number(u64, char),
}

fn parse_prefix(line: &str) -> Prefix {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let mut pos = indent_len;
    while line[pos..].starts_with("> ") || &line[pos..] == ">" {
        pos += if line[pos..].starts_with("> ") { 2 } else { 1 };
    }
    let quote = line[indent_len..pos].to_string();
    let rest = &line[pos..];
    let mut marker = Marker::None;
    let mut len = pos;
    let mut chars = rest.chars();
    if let (Some(b @ ('-' | '*' | '+')), Some(' ')) = (chars.next(), chars.next()) {
        let after = &rest[2..];
        let task = ["[ ] ", "[x] ", "[X] "]
            .iter()
            .find(|t| after.starts_with(*t));
        match task {
            Some(t) => {
                marker = Marker::Task(b, t.as_bytes()[1] != b' ');
                len = pos + 6;
            }
            None => {
                marker = Marker::Bullet(b);
                len = pos + 2;
            }
        }
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if (1..10).contains(&digits) {
            let delim = rest[digits..].chars().next();
            if let Some(d @ ('.' | ')')) = delim {
                if rest[digits + 1..].starts_with(' ') {
                    let n = rest[..digits].parse().unwrap_or(1);
                    marker = Marker::Number(n, d);
                    len = pos + digits + 2;
                }
            }
        }
    }
    Prefix {
        indent: line[..indent_len].to_string(),
        quote,
        marker,
        len,
    }
}

/// `pos` がコードブロック（```）の中か。
fn in_code_block(text: &str, pos: usize) -> bool {
    let mut open = false;
    for line in text[..line_start(text, pos)].lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            open = !open;
        }
    }
    open
}

/// 文書で使われているインデントの単位（タブ、または行頭の空白の最小幅）。既定はタブ。
pub fn indent_unit(text: &str) -> String {
    let mut spaces = usize::MAX;
    for line in text.lines() {
        if line.starts_with('\t') {
            return "\t".to_string();
        }
        let n = line.len() - line.trim_start_matches(' ').len();
        let p = parse_prefix(line);
        if n >= 2 && p.marker != Marker::None {
            spaces = spaces.min(n);
        }
    }
    if spaces == usize::MAX {
        "\t".to_string()
    } else {
        " ".repeat(spaces.min(4))
    }
}

/// Enter: リスト・タスク・番号・引用を次の行に続ける。中身の無い項目なら記号を外す。
/// 対象外なら None（通常の改行に任せる）。
pub fn on_enter(text: &str, sel: Range<usize>) -> Option<Edit> {
    if !sel.is_empty() || in_code_block(text, sel.start) {
        return None;
    }
    let pos = sel.start;
    let (start, end) = (line_start(text, pos), line_end(text, pos));
    let line = &text[start..end];
    let p = parse_prefix(line);
    if p.marker == Marker::None && p.quote.is_empty() {
        return None;
    }
    if pos < start + p.len {
        return None;
    }
    if line[p.len..].trim().is_empty() && pos == end {
        // 空の項目: 入れ子なら1段浅くし、そうでなければ記号を外す
        let unit = indent_unit(text);
        let replacement = if !p.indent.is_empty() {
            let shallower = p.indent.strip_prefix(&unit).unwrap_or("");
            format!("{shallower}{}", &line[p.indent.len()..p.len])
        } else if p.marker != Marker::None {
            p.quote.clone()
        } else {
            // 引用だけの空行: 引用を1段外す
            p.quote
                .strip_suffix("> ")
                .or(p.quote.strip_suffix('>'))
                .unwrap_or("")
                .to_string()
        };
        let caret = start + replacement.len();
        return Some(Edit {
            range: start..end,
            insert: replacement,
            select: caret..caret,
        });
    }
    let marker = match p.marker {
        Marker::None => String::new(),
        Marker::Bullet(b) => format!("{b} "),
        Marker::Task(b, _) => format!("{b} [ ] "),
        Marker::Number(n, d) => format!("{}{d} ", n + 1),
    };
    let insert = format!("\n{}{}{marker}", p.indent, p.quote);
    let caret = pos + insert.len();
    Some(Edit {
        range: pos..pos,
        insert,
        select: caret..caret,
    })
}

/// 選択範囲にかかる行を、行ごとの変換で書き換える。選択は各行の変化に合わせてずらす。
fn map_lines(text: &str, sel: Range<usize>, f: impl Fn(&str) -> String) -> Edit {
    let start = line_start(text, sel.start);
    // 選択の終わりが行頭ちょうどなら、その行は含めない（複数行を選んだときの見た目に合わせる）
    let last = if sel.end > sel.start && sel.end == line_start(text, sel.end) {
        sel.end - 1
    } else {
        sel.end
    };
    let end = line_end(text, last.max(sel.start));
    let mut insert = String::new();
    let (mut new_start, mut new_end) = (sel.start, sel.end);
    let mut delta: isize = 0;
    let mut at = start;
    for (i, line) in text[start..end].split('\n').enumerate() {
        if i > 0 {
            insert.push('\n');
        }
        let changed = f(line);
        let line_delta = changed.len() as isize - line.len() as isize;
        // 行内の位置は、行末側を基準に保つ（行頭に記号を足しても本文上の位置が変わらない）
        let fix = |pos: usize| -> usize {
            let from_end = (at + line.len()).saturating_sub(pos);
            let new_line_end = (at as isize + delta + changed.len() as isize) as usize;
            new_line_end.saturating_sub(from_end.min(changed.len()))
        };
        if (at..=at + line.len()).contains(&sel.start) && new_start == sel.start {
            new_start = fix(sel.start);
        }
        if (at..=at + line.len()).contains(&sel.end) {
            new_end = fix(sel.end);
        }
        insert.push_str(&changed);
        delta += line_delta;
        at += line.len() + 1;
    }
    if sel.end > end {
        new_end = (sel.end as isize + delta) as usize;
    }
    Edit {
        range: start..end,
        insert,
        select: new_start..new_end.max(new_start),
    }
}

/// Tab / Shift+Tab: リスト項目、または複数行の選択をインデント／アウトデントする。
/// 1行の通常の文ではNone（タブ文字の入力に任せる）。
pub fn on_tab(text: &str, sel: Range<usize>, outdent: bool) -> Option<Edit> {
    if in_code_block(text, sel.start) {
        return None;
    }
    let first = &text[line_start(text, sel.start)..line_end(text, sel.start)];
    let multi_line = text[sel.clone()].contains('\n');
    if !multi_line && parse_prefix(first).marker == Marker::None && !outdent {
        return None;
    }
    let unit = indent_unit(text);
    let edit = map_lines(text, sel, |line| {
        if outdent {
            if let Some(rest) = line.strip_prefix(&unit) {
                rest.to_string()
            } else {
                line.trim_start_matches([' ', '\t']).to_string()
            }
        } else if line.is_empty() {
            String::new()
        } else {
            format!("{unit}{line}")
        }
    });
    // Shift+Tabで何も変わらないときは何もしない
    (edit.insert != text[edit.range.clone()]).then_some(edit)
}

/// Ctrl+L: チェックボックスを付ける／切り替える。
pub fn toggle_checkbox(text: &str, sel: Range<usize>) -> Edit {
    map_lines(text, sel, |line| {
        let p = parse_prefix(line);
        let head = &line[..p.indent.len() + p.quote.len()];
        let body = &line[p.len..];
        match p.marker {
            Marker::Task(b, done) => format!("{head}{b} [{}] {body}", if done { ' ' } else { 'x' }),
            Marker::Bullet(b) => format!("{head}{b} [ ] {body}"),
            Marker::Number(n, d) => format!("{head}{n}{d} [ ] {body}"),
            Marker::None => format!("{head}- [ ] {body}"),
        }
    })
}

/// Ctrl+B / Ctrl+I: 選択範囲を `marker` で囲む。既に囲まれていれば外す。
pub fn toggle_wrap(text: &str, sel: Range<usize>, marker: &str) -> Edit {
    let m = marker.len();
    let inner = &text[sel.clone()];
    // 選択のすぐ外側が記号
    if sel.start >= m && text[..sel.start].ends_with(marker) && text[sel.end..].starts_with(marker)
    {
        return Edit {
            range: sel.start - m..sel.end + m,
            insert: inner.to_string(),
            select: sel.start - m..sel.end - m,
        };
    }
    // 選択の内側の両端が記号
    if inner.len() >= 2 * m && inner.starts_with(marker) && inner.ends_with(marker) {
        let stripped = &inner[m..inner.len() - m];
        return Edit {
            range: sel.clone(),
            insert: stripped.to_string(),
            select: sel.start..sel.start + stripped.len(),
        };
    }
    Edit {
        range: sel.clone(),
        insert: format!("{marker}{inner}{marker}"),
        select: sel.start + m..sel.end + m,
    }
}

const PAIRS: [(char, char); 9] = [
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('`', '`'),
    ('*', '*'),
    ('_', '_'),
    ('=', '='),
    ('~', '~'),
];

/// 1文字の入力。選択範囲があれば括弧や記号で囲む。無ければ `(` `[` を閉じ括弧付きで入れ、
/// 直後の閉じ括弧はそのまま上書きして進む。対象外ならNone。
pub fn on_type(text: &str, sel: Range<usize>, ch: char) -> Option<Edit> {
    if in_code_block(text, sel.start) {
        return None;
    }
    if !sel.is_empty() {
        let &(open, close) = PAIRS.iter().find(|(o, _)| *o == ch)?;
        let inner = &text[sel.clone()];
        let o = open.len_utf8();
        return Some(Edit {
            range: sel.clone(),
            insert: format!("{open}{inner}{close}"),
            select: sel.start + o..sel.end + o,
        });
    }
    let pos = sel.start;
    let next = text[pos..].chars().next();
    if matches!(ch, ')' | ']') && next == Some(ch) {
        return Some(Edit {
            range: pos..pos + 1,
            insert: ch.to_string(),
            select: pos + 1..pos + 1,
        });
    }
    let close = match ch {
        '(' => ')',
        '[' => ']',
        _ => return None,
    };
    // 単語の途中では補完しない
    if next.is_some_and(|c| !c.is_whitespace() && !matches!(c, ')' | ']' | '.' | ',')) {
        return None;
    }
    Some(Edit {
        range: pos..pos,
        insert: format!("{ch}{close}"),
        select: pos + 1..pos + 1,
    })
}

/// 選択範囲にURLを貼り付けたら `[選択](URL)` にする。対象外ならNone。
pub fn paste_url(text: &str, sel: Range<usize>, pasted: &str) -> Option<Edit> {
    let url = pasted.trim();
    let is_url = (url.starts_with("http://") || url.starts_with("https://"))
        && !url.contains(char::is_whitespace);
    if sel.is_empty() || !is_url || text[sel.clone()].contains('\n') {
        return None;
    }
    let insert = format!("[{}]({url})", &text[sel.clone()]);
    let caret = sel.start + insert.len();
    Some(Edit {
        range: sel,
        insert,
        select: caret..caret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `|` をカーソル、`«»` を選択範囲として書いたテキストで編集を試す。
    fn run(marked: &str, f: impl Fn(&str, Range<usize>) -> Option<Edit>) -> Option<String> {
        let (text, sel) = unmark(marked);
        let edit = f(&text, sel)?;
        let mut out = text.clone();
        edit.apply(&mut out);
        Some(mark(&out, edit.select))
    }

    fn unmark(marked: &str) -> (String, Range<usize>) {
        if let Some(a) = marked.find('«') {
            let text = marked.replace(['«', '»'], "");
            let b = marked.find('»').unwrap() - '«'.len_utf8();
            return (text, a..b);
        }
        let a = marked.find('|').expect("cursor");
        (marked.replace('|', ""), a..a)
    }

    fn mark(text: &str, sel: Range<usize>) -> String {
        if sel.is_empty() {
            format!("{}|{}", &text[..sel.start], &text[sel.start..])
        } else {
            format!(
                "{}«{}»{}",
                &text[..sel.start],
                &text[sel.clone()],
                &text[sel.end..]
            )
        }
    }

    #[test]
    fn enter_continues_lists() {
        let enter = |s: &str| run(s, on_enter);
        assert_eq!(enter("- a|").unwrap(), "- a\n- |");
        assert_eq!(enter("  * a|").unwrap(), "  * a\n  * |");
        assert_eq!(enter("- [x] done|").unwrap(), "- [x] done\n- [ ] |");
        assert_eq!(enter("9. a|").unwrap(), "9. a\n10. |");
        assert_eq!(enter("1) a|").unwrap(), "1) a\n2) |");
        assert_eq!(enter("> q|").unwrap(), "> q\n> |");
        assert_eq!(enter("> - q|").unwrap(), "> - q\n> - |");
        // 行の途中で押すと、続きを次の項目に送る
        assert_eq!(enter("- ab|cd").unwrap(), "- ab\n- |cd");
        // 空の項目は記号を外す、入れ子は1段浅くする
        assert_eq!(enter("- a\n- |").unwrap(), "- a\n|");
        assert_eq!(enter("- [ ] |").unwrap(), "|");
        assert_eq!(enter("> |").unwrap(), "|");
        assert_eq!(enter("- a\n\t- |").unwrap(), "- a\n- |");
        // 対象外
        assert!(enter("plain|").is_none());
        assert!(enter("-|- a").is_none());
        assert!(enter("```\n- a|\n```").is_none());
        assert!(run("- «a»", on_enter).is_none());
    }

    #[test]
    fn tab_indents_lists_and_selections() {
        let tab = |s: &str| run(s, |t, r| on_tab(t, r, false));
        let untab = |s: &str| run(s, |t, r| on_tab(t, r, true));
        assert_eq!(tab("- a|").unwrap(), "\t- a|");
        assert_eq!(tab("- a\n  - b|").unwrap(), "- a\n    - b|");
        assert!(tab("plain|").is_none());
        assert_eq!(untab("\t- a|").unwrap(), "- a|");
        assert!(untab("- a|").is_none());
        // 複数行の選択はまとめて動かし、選択も保つ
        assert_eq!(tab("«x\ny»").unwrap(), "\t«x\n\ty»");
        assert_eq!(untab("\t«x\n\ty»").unwrap(), "«x\ny»");
        // 選択の終わりが行頭なら、その行は動かさない
        assert_eq!(tab("«x\n»y").unwrap(), "\t«x\n»y");
    }

    #[test]
    fn checkbox_cycle() {
        let cb = |s: &str| run(s, |t, r| Some(toggle_checkbox(t, r))).unwrap();
        assert_eq!(cb("todo|"), "- [ ] todo|");
        assert_eq!(cb("- [ ] todo|"), "- [x] todo|");
        assert_eq!(cb("- [x] to|do"), "- [ ] to|do");
        assert_eq!(cb("  - item|"), "  - [ ] item|");
        assert_eq!(cb("|"), "- [ ] |");
        assert_eq!(cb("«a\nb»"), "- [ ] «a\n- [ ] b»");
    }

    #[test]
    fn wrap_and_unwrap() {
        let bold = |s: &str| run(s, |t, r| Some(toggle_wrap(t, r, "**"))).unwrap();
        assert_eq!(bold("x «太字» y"), "x **«太字»** y");
        assert_eq!(bold("x **«太字»** y"), "x «太字» y");
        assert_eq!(bold("x «**太字**» y"), "x «太字» y");
        assert_eq!(bold("x | y"), "x **|** y");
    }

    #[test]
    fn typing_pairs() {
        let ty = |s: &str, c| run(s, |t, r| on_type(t, r, c));
        assert_eq!(ty("«語»", '[').unwrap(), "[«語»]");
        assert_eq!(ty("«語»", '*').unwrap(), "*«語»*");
        assert_eq!(ty("a |", '(').unwrap(), "a (|)");
        assert_eq!(ty("[|]", '[').unwrap(), "[[|]]");
        assert_eq!(ty("(|)", ')').unwrap(), "()|");
        assert!(ty("a|b", '(').is_none());
        assert!(ty("a|", 'x').is_none());
        assert!(ty("```\n|\n```", '(').is_none());
    }

    #[test]
    fn url_paste() {
        let paste = |s: &str, p: &str| run(s, |t, r| paste_url(t, r, p));
        assert_eq!(
            paste("see «docs»", "https://x.y/a").unwrap(),
            "see [docs](https://x.y/a)|"
        );
        assert!(paste("see |", "https://x.y").is_none());
        assert!(paste("«a»", "not a url").is_none());
    }

    #[test]
    fn indent_detection() {
        assert_eq!(indent_unit("- a\n  - b"), "  ");
        assert_eq!(indent_unit("- a\n\t- b"), "\t");
        assert_eq!(indent_unit("plain"), "\t");
    }
}
