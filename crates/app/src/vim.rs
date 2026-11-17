//! Vim風のモーダル編集エンジン。
//!
//! 本文（`String`）とカーソル（バイト位置）を受け取ってキーを1つずつ処理し、本文とカーソルを書き換える。
//! GUIに依存しないので、`|` でカーソルを書いたテキストでテストできる。
//! 挿入モード中の通常の文字入力はエディタ（TextEdit）に任せ、エンジンはEscだけを受け取る
//! （`.` の繰り返しで挿入を再生するときだけ、エンジン自身が文字を入れる）。

use crate::edit::indent_unit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Esc,
    Enter,
    Backspace,
    /// Ctrl+R（やり直し）
    CtrlR,
    /// Ctrl+D / Ctrl+U（半画面移動）
    CtrlD,
    CtrlU,
}

/// エディタ側で行ってほしいこと。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    Save,
    Close,
    SaveClose,
    /// `/`（true）または `?`（false）で検索バーを開く
    OpenFind(bool),
    FindNext,
    FindPrev,
    /// `*` / `#`: カーソル下の語を検索する（trueなら前方）
    SearchWord(String, bool),
    Undo(usize),
    Redo(usize),
    /// `:noh`
    ClearSearch,
    /// 知らない `:` コマンド
    Unknown(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Effects {
    /// 本文を変えたか
    pub changed: bool,
    pub cmd: Option<Cmd>,
    /// ヤンク（コピー）した文字列。システムのクリップボードにも入れる
    pub yank: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Register {
    text: String,
    linewise: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Exclusive,
    Inclusive,
    Linewise,
}

pub struct Vim {
    pub mode: Mode,
    /// ビジュアルモードの起点
    pub anchor: usize,
    keys: Vec<Key>,
    register: Register,
    /// 上下移動で保つ列（文字数）
    want_col: Option<usize>,
    last_find: Option<(char, bool, bool)>,
    last_change: Vec<Key>,
    /// 挿入モードに入ったコマンドのキーと、挿入を始めた位置・その時の本文の長さ
    insert_record: Option<(Vec<Key>, usize, usize)>,
    replaying: bool,
    /// `:` コマンドの入力中
    pub cmdline: Option<String>,
}

impl Default for Vim {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            anchor: 0,
            keys: Vec::new(),
            register: Register::default(),
            want_col: None,
            last_find: None,
            last_change: Vec::new(),
            insert_record: None,
            replaying: false,
            cmdline: None,
        }
    }
}

// ---- 位置の計算（すべてバイト位置） ----

fn ch(t: &str, p: usize) -> Option<char> {
    t.get(p..)?.chars().next()
}
fn next(t: &str, p: usize) -> usize {
    ch(t, p).map_or(p, |c| p + c.len_utf8())
}
fn prev(t: &str, p: usize) -> usize {
    t[..p].chars().next_back().map_or(p, |c| p - c.len_utf8())
}
fn line_start(t: &str, p: usize) -> usize {
    t[..p].rfind('\n').map_or(0, |i| i + 1)
}
fn line_end(t: &str, p: usize) -> usize {
    t[p..].find('\n').map_or(t.len(), |i| p + i)
}
fn col(t: &str, p: usize) -> usize {
    t[line_start(t, p)..p].chars().count()
}
fn first_nonblank(t: &str, p: usize) -> usize {
    let s = line_start(t, p);
    let e = line_end(t, s);
    s + t[s..e].len() - t[s..e].trim_start_matches([' ', '\t']).len()
}
fn line_count(t: &str) -> usize {
    t.matches('\n').count() + 1
}
/// `n` 行目（0始まり）の先頭。
fn nth_line(t: &str, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    t.match_indices('\n')
        .nth(n - 1)
        .map_or_else(|| line_start(t, t.len()), |(i, _)| i + 1)
}
fn line_no(t: &str, p: usize) -> usize {
    t[..p].matches('\n').count()
}
/// 行内の `c` 列目（行が短ければ最後の文字）。
fn at_col(t: &str, ls: usize, c: usize) -> usize {
    let le = line_end(t, ls);
    let mut p = ls;
    for _ in 0..c {
        let n = next(t, p);
        if n >= le {
            break;
        }
        p = n;
    }
    if p >= le && le > ls {
        prev(t, le)
    } else {
        p
    }
}
/// ノーマルモードではカーソルを改行の上に置かない（空行を除く）。
pub fn clamp_normal(t: &str, p: usize) -> usize {
    let p = p.min(t.len());
    let le = line_end(t, p);
    if p >= le && le > line_start(t, p) {
        prev(t, le)
    } else {
        p
    }
}
fn is_blank_line(t: &str, p: usize) -> bool {
    t[line_start(t, p)..line_end(t, p)].trim().is_empty()
}

/// 単語の種類。日本語は文字の種類（ひらがな・カタカナ・漢字）が変わる所を単語の境目とする。
fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        return 0;
    }
    if big {
        return 1;
    }
    match c {
        c if c.is_ascii_alphanumeric() || c == '_' => 1,
        c if c.is_ascii() => 2,
        '\u{3040}'..='\u{309F}' => 3,
        '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}' => 4,
        '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々' => 5,
        '\u{3000}'..='\u{303F}' | '\u{FF00}'..='\u{FF65}' => 6,
        _ => 7,
    }
}

fn word_fwd(t: &str, p: usize, big: bool) -> usize {
    let mut i = p;
    let Some(c) = ch(t, i) else { return p };
    let k = class(c, big);
    if k != 0 {
        while ch(t, i).is_some_and(|c| class(c, big) == k) {
            i = next(t, i);
        }
    }
    while let Some(c) = ch(t, i) {
        if c == '\n' {
            let n = next(t, i);
            // 空行は単語として止まる
            if ch(t, n) == Some('\n') {
                return n;
            }
            i = n;
            continue;
        }
        if !c.is_whitespace() {
            break;
        }
        i = next(t, i);
    }
    i
}

fn word_end(t: &str, p: usize, big: bool) -> usize {
    let mut i = next(t, p);
    while ch(t, i).is_some_and(char::is_whitespace) {
        i = next(t, i);
    }
    let Some(c) = ch(t, i) else {
        return prev(t, t.len()).max(p);
    };
    let k = class(c, big);
    while ch(t, next(t, i)).is_some_and(|c| class(c, big) == k) {
        i = next(t, i);
    }
    i
}

fn word_back(t: &str, p: usize, big: bool) -> usize {
    if p == 0 {
        return 0;
    }
    let mut i = prev(t, p);
    while i > 0 && ch(t, i).is_some_and(char::is_whitespace) {
        i = prev(t, i);
    }
    let Some(c) = ch(t, i) else { return i };
    let k = class(c, big);
    while i > 0 && ch(t, prev(t, i)).is_some_and(|c| class(c, big) == k) {
        i = prev(t, i);
    }
    i
}

/// `repeat` は `;` `,` での繰り返し（t / T がすぐ隣の文字で止まり続けないよう、1文字先から探す）。
fn find_in_line(
    t: &str,
    p: usize,
    c: char,
    forward: bool,
    till: bool,
    count: usize,
    repeat: bool,
) -> Option<usize> {
    let (ls, le) = (line_start(t, p), line_end(t, p));
    let mut i = p;
    for k in 0..count {
        let skip = till && (repeat || k > 0);
        if forward {
            let from = next(t, i);
            let start = if skip { next(t, from) } else { from }.min(le);
            i = start + t[start..le].find(c)?;
        } else {
            let end = if skip { prev(t, i) } else { i };
            i = ls + t[ls..end.max(ls)].rfind(c)?;
        }
    }
    Some(match (forward, till) {
        (true, true) => prev(t, i),
        (false, true) => next(t, i),
        _ => i,
    })
}

fn paragraph(t: &str, p: usize, forward: bool) -> usize {
    let mut line = line_no(t, p);
    let total = line_count(t);
    // 今いる空行の並びを抜けてから、次の空行を探す
    let blank = |n: usize| is_blank_line(t, nth_line(t, n));
    if forward {
        while line + 1 < total && blank(line) {
            line += 1;
        }
        while line + 1 < total && !blank(line) {
            line += 1;
        }
        if blank(line) {
            nth_line(t, line)
        } else {
            t.len()
        }
    } else {
        while line > 0 && blank(line) {
            line -= 1;
        }
        while line > 0 && !blank(line) {
            line -= 1;
        }
        nth_line(t, line)
    }
}

const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

fn match_pair(t: &str, p: usize) -> Option<usize> {
    let le = line_end(t, p);
    let (start, c) = t[p..le]
        .char_indices()
        .find(|(_, c)| PAIRS.iter().any(|(o, cl)| o == c || cl == c))
        .map(|(i, c)| (p + i, c))?;
    if let Some(&(open, close)) = PAIRS.iter().find(|(o, _)| *o == c) {
        let mut depth = 0;
        for (i, c) in t[start..].char_indices() {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i);
                }
            }
        }
        None
    } else {
        let &(open, close) = PAIRS.iter().find(|(_, cl)| *cl == c)?;
        let mut depth = 0;
        for (i, c) in t[..=start].char_indices().rev() {
            if c == close {
                depth += 1;
            } else if c == open {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
        }
        None
    }
}

/// 対応する括弧の内側（`inner`）または括弧ごとの範囲。
fn bracket_object(
    t: &str,
    p: usize,
    open: char,
    close: char,
    inner: bool,
) -> Option<(usize, usize)> {
    let mut depth = 0;
    let mut left = None;
    let upto = if ch(t, p) == Some(open) {
        next(t, p)
    } else {
        p
    };
    for (i, c) in t[..upto].char_indices().rev() {
        if c == close && i != p {
            depth += 1;
        } else if c == open {
            if depth == 0 {
                left = Some(i);
                break;
            }
            depth -= 1;
        }
    }
    let left = left?;
    let mut depth = 0;
    for (i, c) in t[left..].char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                let right = left + i;
                return Some(if inner {
                    (left + open.len_utf8(), right)
                } else {
                    (left, right + close.len_utf8())
                });
            }
        }
    }
    None
}

fn quote_object(t: &str, p: usize, q: char, inner: bool) -> Option<(usize, usize)> {
    let (ls, le) = (line_start(t, p), line_end(t, p));
    let quotes: Vec<usize> = t[ls..le].match_indices(q).map(|(i, _)| ls + i).collect();
    let pair = quotes
        .chunks_exact(2)
        .find(|w| w[0] <= p && p <= w[1])
        .or_else(|| quotes.chunks_exact(2).find(|w| w[0] > p))?;
    let (a, b) = (pair[0], pair[1]);
    Some(if inner {
        (a + q.len_utf8(), b)
    } else {
        (a, b + q.len_utf8())
    })
}

fn word_object(t: &str, p: usize, big: bool, around: bool) -> Option<(usize, usize)> {
    let c = ch(t, p).filter(|c| *c != '\n')?;
    let k = class(c, big);
    let (ls, le) = (line_start(t, p), line_end(t, p));
    let mut a = p;
    while a > ls && ch(t, prev(t, a)).is_some_and(|c| class(c, big) == k) {
        a = prev(t, a);
    }
    let mut b = next(t, p);
    while b < le && ch(t, b).is_some_and(|c| class(c, big) == k) {
        b = next(t, b);
    }
    if around && k != 0 {
        let mut e = b;
        while e < le && ch(t, e).is_some_and(|c| c == ' ' || c == '\t') {
            e = next(t, e);
        }
        if e > b {
            b = e;
        } else {
            while a > ls && ch(t, prev(t, a)).is_some_and(|c| c == ' ' || c == '\t') {
                a = prev(t, a);
            }
        }
    }
    Some((a, b))
}

fn paragraph_object(t: &str, p: usize, around: bool) -> (usize, usize) {
    let total = line_count(t);
    let blank = |n: usize| is_blank_line(t, nth_line(t, n));
    let here = line_no(t, p);
    let kind = blank(here);
    let (mut a, mut b) = (here, here);
    while a > 0 && blank(a - 1) == kind {
        a -= 1;
    }
    while b + 1 < total && blank(b + 1) == kind {
        b += 1;
    }
    if around {
        while b + 1 < total && blank(b + 1) != kind {
            b += 1;
        }
    }
    (nth_line(t, a), line_end(t, nth_line(t, b)))
}

/// 行単位の範囲を、改行も含めた削除範囲に広げる。
fn linewise_range(t: &str, a: usize, b: usize) -> (usize, usize) {
    let start = line_start(t, a.min(b));
    let end = line_end(t, a.max(b));
    if end < t.len() {
        (start, end + 1)
    } else if start > 0 {
        // 最終行は、前の行末の改行ごと消す
        (start - 1, end)
    } else {
        (start, end)
    }
}

enum Parsed {
    Incomplete,
    Invalid,
    Done(Action),
}

#[derive(Debug, Clone)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordFwd(bool),
    WordBack(bool),
    WordEnd(bool),
    LineStart,
    FirstNonBlank,
    LineEnd,
    /// gg（行番号の指定があればその行）
    Top,
    /// G
    Bottom,
    Find(char, bool, bool),
    RepeatFind(bool),
    ParaFwd,
    ParaBack,
    Match,
    HalfDown,
    HalfUp,
}

#[derive(Debug, Clone)]
enum Object {
    Word(bool, bool),
    Paragraph(bool),
    Bracket(char, char, bool),
    Quote(char, bool),
}

#[derive(Debug, Clone)]
enum Target {
    Motion(Motion),
    Object(Object),
    /// dd, cc, yy, >>, <<
    Line,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
    Lower,
    Upper,
    Toggle,
}

#[derive(Debug, Clone)]
enum Action {
    Move(Motion),
    Operate(Op, Target),
    Insert(char),
    DeleteChar(bool),
    Replace(char),
    Put(bool),
    Join,
    ToggleCase,
    Visual(bool),
    Repeat,
    Undo,
    Find(bool),
    FindNext(bool),
    SearchWord(bool),
    Cmdline,
}

/// キー列（数値プレフィックスを除く）を解釈する。
fn parse(keys: &[char]) -> Parsed {
    use Parsed::*;
    let Some(&first) = keys.first() else {
        return Incomplete;
    };
    let rest = &keys[1..];
    if let Some(op) = op_of(keys) {
        let op_len = if first == 'g' { 2 } else { 1 };
        let rest = &keys[op_len..];
        // 演算子のあとの数値は飛ばす（倍数はcountで扱う）
        let digits = rest
            .iter()
            .take_while(|c| c.is_ascii_digit() && **c != '0')
            .count();
        let rest = &rest[digits..];
        let Some(&c) = rest.first() else {
            return Incomplete;
        };
        let same = match op {
            Op::Lower => c == 'u' || c == 'g' && rest.get(1) == Some(&'u'),
            Op::Upper => c == 'U',
            Op::Toggle => c == '~',
            _ => c == first,
        };
        if same {
            return Done(Action::Operate(op, Target::Line));
        }
        if c == 'i' || c == 'a' {
            let Some(&o) = rest.get(1) else {
                return Incomplete;
            };
            let inner = c == 'i';
            let obj = match o {
                'w' => Object::Word(false, !inner),
                'W' => Object::Word(true, !inner),
                'p' => Object::Paragraph(!inner),
                '(' | ')' | 'b' => Object::Bracket('(', ')', inner),
                '[' | ']' => Object::Bracket('[', ']', inner),
                '{' | '}' | 'B' => Object::Bracket('{', '}', inner),
                '<' | '>' => Object::Bracket('<', '>', inner),
                '"' | '\'' | '`' => Object::Quote(o, inner),
                _ => return Invalid,
            };
            return Done(Action::Operate(op, Target::Object(obj)));
        }
        return match motion(rest) {
            Done(Action::Move(m)) => Done(Action::Operate(op, Target::Motion(m))),
            Incomplete => Incomplete,
            _ => Invalid,
        };
    }
    match first {
        'i' | 'a' | 'I' | 'A' | 'o' | 'O' | 's' | 'S' | 'C' => Done(Action::Insert(first)),
        'x' => Done(Action::DeleteChar(true)),
        'X' => Done(Action::DeleteChar(false)),
        'D' => Done(Action::Operate(Op::Delete, Target::Motion(Motion::LineEnd))),
        'Y' => Done(Action::Operate(Op::Yank, Target::Line)),
        'r' => rest
            .first()
            .map_or(Incomplete, |&c| Done(Action::Replace(c))),
        'p' => Done(Action::Put(true)),
        'P' => Done(Action::Put(false)),
        'J' => Done(Action::Join),
        '~' => Done(Action::ToggleCase),
        'v' => Done(Action::Visual(false)),
        'V' => Done(Action::Visual(true)),
        '.' => Done(Action::Repeat),
        'u' => Done(Action::Undo),
        '/' => Done(Action::Find(true)),
        '?' => Done(Action::Find(false)),
        'n' => Done(Action::FindNext(true)),
        'N' => Done(Action::FindNext(false)),
        '*' => Done(Action::SearchWord(true)),
        '#' => Done(Action::SearchWord(false)),
        ':' => Done(Action::Cmdline),
        _ => motion(keys),
    }
}

fn op_of(keys: &[char]) -> Option<Op> {
    Some(match keys {
        ['d', ..] => Op::Delete,
        ['c', ..] => Op::Change,
        ['y', ..] => Op::Yank,
        ['>', ..] => Op::Indent,
        ['<', ..] => Op::Outdent,
        ['g', 'u', ..] => Op::Lower,
        ['g', 'U', ..] => Op::Upper,
        ['g', '~', ..] => Op::Toggle,
        _ => return None,
    })
}

fn motion(keys: &[char]) -> Parsed {
    use Motion::*;
    let Some(&c) = keys.first() else {
        return Parsed::Incomplete;
    };
    let m = match c {
        'h' => Left,
        'l' | ' ' => Right,
        'k' => Up,
        'j' => Down,
        'w' => WordFwd(false),
        'W' => WordFwd(true),
        'b' => WordBack(false),
        'B' => WordBack(true),
        'e' => WordEnd(false),
        'E' => WordEnd(true),
        '0' => LineStart,
        '^' => FirstNonBlank,
        '$' => LineEnd,
        'G' => Bottom,
        '}' => ParaFwd,
        '{' => ParaBack,
        '%' => Match,
        ';' => RepeatFind(false),
        ',' => RepeatFind(true),
        'g' => match keys.get(1) {
            None => return Parsed::Incomplete,
            Some('g') => Top,
            Some(_) => return Parsed::Invalid,
        },
        'f' | 'F' | 't' | 'T' => match keys.get(1) {
            None => return Parsed::Incomplete,
            Some(&x) => Find(x, c == 'f' || c == 't', c == 't' || c == 'T'),
        },
        _ => return Parsed::Invalid,
    };
    Parsed::Done(Action::Move(m))
}

impl Vim {
    pub fn pending(&self) -> String {
        self.keys
            .iter()
            .filter_map(|k| match k {
                Key::Char(c) => Some(*c),
                _ => None,
            })
            .collect()
    }

    /// ビジュアルモードの選択範囲（バイト位置、終わりは含まない）。
    pub fn selection(&self, t: &str, cursor: usize) -> Option<(usize, usize)> {
        match self.mode {
            Mode::Visual => {
                let (a, b) = (self.anchor.min(cursor), self.anchor.max(cursor));
                Some((a, next(t, b).min(t.len()).max(b)))
            }
            Mode::VisualLine => {
                let (a, b) = (self.anchor.min(cursor), self.anchor.max(cursor));
                Some((line_start(t, a), line_end(t, b)))
            }
            _ => None,
        }
    }

    /// キーを1つ処理する。
    pub fn handle(&mut self, t: &mut String, cursor: &mut usize, key: Key) -> Effects {
        let mut fx = Effects::default();
        *cursor = (*cursor).min(t.len());
        if let Some(line) = self.cmdline.as_mut() {
            match key {
                Key::Char(c) => line.push(c),
                Key::Backspace => {
                    if line.pop().is_none() {
                        self.cmdline = None;
                    }
                }
                Key::Esc => self.cmdline = None,
                Key::Enter => {
                    let line = self.cmdline.take().unwrap_or_default();
                    self.ex(line.trim(), t, cursor, &mut fx);
                }
                _ => {}
            }
            return fx;
        }
        if self.mode == Mode::Insert {
            self.insert_key(t, cursor, key, &mut fx);
            return fx;
        }
        match key {
            Key::Esc => {
                self.keys.clear();
                if matches!(self.mode, Mode::Visual | Mode::VisualLine) {
                    self.mode = Mode::Normal;
                }
                return fx;
            }
            Key::CtrlR => {
                self.keys.clear();
                fx.cmd = Some(Cmd::Redo(1));
                return fx;
            }
            Key::CtrlD | Key::CtrlU => {
                self.keys.clear();
                let m = if key == Key::CtrlD {
                    Motion::HalfDown
                } else {
                    Motion::HalfUp
                };
                self.run(Action::Move(m), 1, t, cursor, &mut fx);
                return fx;
            }
            Key::Enter => {
                self.keys.clear();
                self.run(Action::Move(Motion::Down), 1, t, cursor, &mut fx);
                *cursor = first_nonblank(t, *cursor);
                return fx;
            }
            Key::Backspace => {
                self.keys.clear();
                self.run(Action::Move(Motion::Left), 1, t, cursor, &mut fx);
                return fx;
            }
            Key::Char(_) => self.keys.push(key),
        }
        if matches!(self.mode, Mode::Visual | Mode::VisualLine) && self.keys.len() == 1 {
            let Key::Char(c) = key else { unreachable!() };
            let op = match c {
                'd' | 'x' => Some(Op::Delete),
                'c' | 's' => Some(Op::Change),
                'y' => Some(Op::Yank),
                '>' => Some(Op::Indent),
                '<' => Some(Op::Outdent),
                'u' => Some(Op::Lower),
                'U' => Some(Op::Upper),
                '~' => Some(Op::Toggle),
                _ => None,
            };
            if let Some(op) = op {
                self.keys.clear();
                self.visual_op(op, t, cursor, &mut fx);
                return fx;
            }
            if c == 'o' {
                self.keys.clear();
                std::mem::swap(&mut self.anchor, cursor);
                return fx;
            }
        }
        let chars: Vec<char> = self.pending().chars().collect();
        // 先頭の数値（0は数値の途中だけ）
        let digits = chars
            .iter()
            .enumerate()
            .take_while(|(i, c)| c.is_ascii_digit() && (*i > 0 || **c != '0'))
            .count();
        let count: Option<usize> = chars[..digits].iter().collect::<String>().parse().ok();
        // 演算子のあとの数値も倍数にする（2d3w = 6単語）
        let body = &chars[digits..];
        let inner_count = op_of(body).and_then(|op| {
            let skip = if matches!(op, Op::Lower | Op::Upper | Op::Toggle) {
                2
            } else {
                1
            };
            let s: String = body
                .get(skip..)?
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            s.parse::<usize>().ok()
        });
        match parse(body) {
            Parsed::Incomplete => {}
            Parsed::Invalid => self.keys.clear(),
            Parsed::Done(action) => {
                let keys = std::mem::take(&mut self.keys);
                let n = count.unwrap_or(1) * inner_count.unwrap_or(1);
                // gg / G に数値が付いたら行番号
                let action = match (action, count) {
                    (Action::Move(Motion::Top | Motion::Bottom), Some(line)) => {
                        *cursor = first_nonblank(t, nth_line(t, line.saturating_sub(1)));
                        self.want_col = None;
                        return fx;
                    }
                    (a, _) => a,
                };
                let before_mode = self.mode;
                let records = self.records(&action);
                self.run(action, n, t, cursor, &mut fx);
                if records && !self.replaying && before_mode == Mode::Normal {
                    if self.mode == Mode::Insert {
                        if let Some(rec) = self.insert_record.as_mut() {
                            rec.0 = keys;
                        }
                    } else {
                        self.last_change = keys;
                    }
                }
            }
        }
        fx
    }

    fn records(&self, a: &Action) -> bool {
        match a {
            Action::Operate(op, _) => *op != Op::Yank,
            Action::Insert(_)
            | Action::DeleteChar(_)
            | Action::Replace(_)
            | Action::Put(_)
            | Action::Join
            | Action::ToggleCase => true,
            _ => false,
        }
    }

    fn enter_insert(&mut self, t: &str, at: usize) {
        self.mode = Mode::Insert;
        self.insert_record = Some((Vec::new(), at, t.len()));
    }

    fn insert_key(&mut self, t: &mut String, cursor: &mut usize, key: Key, fx: &mut Effects) {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                if let Some((mut keys, start, len)) = self.insert_record.take() {
                    if !self.replaying {
                        // 挿入した文字列を記録して、`.` で再生できるようにする
                        let grown = t.len().saturating_sub(len);
                        let typed = t
                            .get(start..start + grown)
                            .filter(|_| *cursor == start + grown);
                        if let Some(typed) = typed {
                            keys.extend(typed.chars().map(|c| {
                                if c == '\n' {
                                    Key::Enter
                                } else {
                                    Key::Char(c)
                                }
                            }));
                        }
                        keys.push(Key::Esc);
                        self.last_change = keys;
                    }
                }
                if *cursor > line_start(t, *cursor) {
                    *cursor = prev(t, *cursor);
                }
            }
            // ここから下は `.` の再生のときだけ使う（普段の入力はエディタが行う）
            Key::Char(c) => {
                t.insert(*cursor, c);
                *cursor += c.len_utf8();
                fx.changed = true;
            }
            Key::Enter => {
                t.insert(*cursor, '\n');
                *cursor += 1;
                fx.changed = true;
            }
            Key::Backspace if *cursor > 0 => {
                let p = prev(t, *cursor);
                t.replace_range(p..*cursor, "");
                *cursor = p;
                fx.changed = true;
            }
            _ => {}
        }
    }

    /// 移動先と種類。
    fn motion_target(&mut self, m: &Motion, n: usize, t: &str, p: usize) -> Option<(usize, Kind)> {
        use Motion::*;
        let vertical = matches!(m, Up | Down | HalfDown | HalfUp);
        if !vertical {
            self.want_col = None;
        }
        let r = match m {
            Left => {
                let ls = line_start(t, p);
                let mut i = p;
                for _ in 0..n {
                    if i > ls {
                        i = prev(t, i);
                    }
                }
                (i, Kind::Exclusive)
            }
            Right => {
                let le = line_end(t, p);
                let mut i = p;
                for _ in 0..n {
                    if next(t, i) <= le {
                        i = next(t, i);
                    }
                }
                (i, Kind::Exclusive)
            }
            Up | Down | HalfDown | HalfUp => {
                let want = *self.want_col.get_or_insert_with(|| col(t, p));
                let line = line_no(t, p);
                let n = if matches!(m, HalfDown | HalfUp) {
                    15 * n
                } else {
                    n
                };
                let target = if matches!(m, Down | HalfDown) {
                    (line + n).min(line_count(t) - 1)
                } else {
                    line.saturating_sub(n)
                };
                if target == line && !matches!(m, HalfDown | HalfUp) {
                    return None;
                }
                (at_col(t, nth_line(t, target), want), Kind::Linewise)
            }
            WordFwd(big) => {
                let mut i = p;
                for _ in 0..n {
                    i = word_fwd(t, i, *big);
                }
                (i, Kind::Exclusive)
            }
            WordBack(big) => {
                let mut i = p;
                for _ in 0..n {
                    i = word_back(t, i, *big);
                }
                (i, Kind::Exclusive)
            }
            WordEnd(big) => {
                let mut i = p;
                for _ in 0..n {
                    i = word_end(t, i, *big);
                }
                (i, Kind::Inclusive)
            }
            LineStart => (line_start(t, p), Kind::Exclusive),
            FirstNonBlank => (first_nonblank(t, p), Kind::Exclusive),
            LineEnd => {
                let mut i = p;
                for _ in 1..n {
                    i = next(t, line_end(t, i)).min(t.len());
                }
                (
                    prev(t, line_end(t, i)).max(line_start(t, i)),
                    Kind::Inclusive,
                )
            }
            Top => (first_nonblank(t, 0), Kind::Linewise),
            Bottom => (first_nonblank(t, line_start(t, t.len())), Kind::Linewise),
            Find(c, fwd, till) => {
                self.last_find = Some((*c, *fwd, *till));
                let target = find_in_line(t, p, *c, *fwd, *till, n, false)?;
                (
                    target,
                    if *fwd {
                        Kind::Inclusive
                    } else {
                        Kind::Exclusive
                    },
                )
            }
            RepeatFind(reverse) => {
                let (c, fwd, till) = self.last_find?;
                let fwd = fwd != *reverse;
                let target = find_in_line(t, p, c, fwd, till, n, true)?;
                (
                    target,
                    if fwd {
                        Kind::Inclusive
                    } else {
                        Kind::Exclusive
                    },
                )
            }
            ParaFwd => {
                let mut i = p;
                for _ in 0..n {
                    i = paragraph(t, i, true);
                }
                (i, Kind::Exclusive)
            }
            ParaBack => {
                let mut i = p;
                for _ in 0..n {
                    i = paragraph(t, i, false);
                }
                (i, Kind::Exclusive)
            }
            Match => (match_pair(t, p)?, Kind::Inclusive),
        };
        Some(r)
    }

    fn object_range(&self, o: &Object, t: &str, p: usize) -> Option<(usize, usize, bool)> {
        Some(match o {
            Object::Word(big, around) => {
                let (a, b) = word_object(t, p, *big, *around)?;
                (a, b, false)
            }
            Object::Paragraph(around) => {
                let (a, b) = paragraph_object(t, p, *around);
                (a, b, true)
            }
            Object::Bracket(o, c, inner) => {
                let (a, b) = bracket_object(t, p, *o, *c, *inner)?;
                (a, b, false)
            }
            Object::Quote(q, inner) => {
                let (a, b) = quote_object(t, p, *q, *inner)?;
                (a, b, false)
            }
        })
    }

    fn run(
        &mut self,
        action: Action,
        n: usize,
        t: &mut String,
        cursor: &mut usize,
        fx: &mut Effects,
    ) {
        let p = *cursor;
        let visual = matches!(self.mode, Mode::Visual | Mode::VisualLine);
        match action {
            Action::Move(m) => {
                if let Some((target, _)) = self.motion_target(&m, n, t, p) {
                    *cursor = if visual {
                        target.min(t.len())
                    } else {
                        clamp_normal(t, target)
                    };
                }
            }
            Action::Operate(op, target) if visual => {
                let _ = target;
                self.visual_op(op, t, cursor, fx);
            }
            Action::Operate(op, target) => {
                let range = match &target {
                    Target::Line => {
                        let last = nth_line(t, (line_no(t, p) + n - 1).min(line_count(t) - 1));
                        Some((line_start(t, p), line_end(t, last), true))
                    }
                    Target::Object(o) => self.object_range(o, t, p),
                    Target::Motion(m) => {
                        let mut m = m.clone();
                        // cw は ce と同じ（単語の後ろの空白は残す）
                        if op == Op::Change
                            && matches!(m, Motion::WordFwd(_))
                            && ch(t, p).is_some_and(|c| !c.is_whitespace())
                        {
                            m = match m {
                                Motion::WordFwd(big) => Motion::WordEnd(big),
                                other => other,
                            };
                            if let Some(c) = ch(t, p) {
                                let big = matches!(m, Motion::WordEnd(true));
                                // カーソルが単語の最後の文字なら、その文字だけ
                                if !ch(t, next(t, p))
                                    .is_some_and(|d| class(d, big) == class(c, big))
                                    && n == 1
                                {
                                    self.apply(op, p, next(t, p), false, t, cursor, fx);
                                    return;
                                }
                            }
                        }
                        self.motion_target(&m, n, t, p).map(|(target, kind)| {
                            let (a, b) = (p.min(target), p.max(target));
                            match kind {
                                Kind::Linewise => (line_start(t, a), line_end(t, b), true),
                                Kind::Inclusive => (
                                    a,
                                    next(t, b).min(line_end(t, b).max(next(t, b).min(t.len()))),
                                    false,
                                ),
                                Kind::Exclusive => {
                                    // dw などで行をまたいだら、その行の終わりまでにする
                                    let b = if matches!(m, Motion::WordFwd(_))
                                        && line_no(t, b) > line_no(t, a)
                                    {
                                        line_end(t, a)
                                    } else {
                                        b
                                    };
                                    (a, b, false)
                                }
                            }
                        })
                    }
                };
                if let Some((a, b, linewise)) = range {
                    let keep = matches!(target, Target::Line) && op == Op::Yank;
                    self.apply(op, a, b, linewise, t, cursor, fx);
                    if keep {
                        *cursor = p;
                    }
                }
            }
            Action::Insert(c) => {
                let (ls, le) = (line_start(t, p), line_end(t, p));
                let at = match c {
                    'i' => p,
                    'a' => next(t, p).min(le),
                    'I' => first_nonblank(t, p),
                    'A' => le,
                    'o' => {
                        let indent = t[ls..first_nonblank(t, p)].to_string();
                        t.insert_str(le, &format!("\n{indent}"));
                        fx.changed = true;
                        le + 1 + indent.len()
                    }
                    'O' => {
                        let indent = t[ls..first_nonblank(t, p)].to_string();
                        t.insert_str(ls, &format!("{indent}\n"));
                        fx.changed = true;
                        ls + indent.len()
                    }
                    's' => {
                        let mut e = p;
                        for _ in 0..n {
                            if e < le {
                                e = next(t, e);
                            }
                        }
                        self.yank_text(&t[p..e], false, fx);
                        t.replace_range(p..e, "");
                        fx.changed |= e > p;
                        p
                    }
                    'S' => {
                        let fnb = first_nonblank(t, p);
                        self.yank_text(&t[ls..le], true, fx);
                        t.replace_range(fnb..le, "");
                        fx.changed = true;
                        fnb
                    }
                    'C' => {
                        self.yank_text(&t[p..le], false, fx);
                        t.replace_range(p..le, "");
                        fx.changed |= le > p;
                        p
                    }
                    _ => p,
                };
                *cursor = at;
                self.enter_insert(t, at);
            }
            Action::DeleteChar(forward) => {
                let (ls, le) = (line_start(t, p), line_end(t, p));
                let (mut a, mut b) = (p, p);
                for _ in 0..n {
                    if forward {
                        if b < le {
                            b = next(t, b);
                        }
                    } else if a > ls {
                        a = prev(t, a);
                    }
                }
                if b > a {
                    self.yank_text(&t[a..b], false, fx);
                    t.replace_range(a..b, "");
                    fx.changed = true;
                }
                *cursor = clamp_normal(t, a);
            }
            Action::Replace(c) => {
                let le = line_end(t, p);
                let mut b = p;
                for _ in 0..n {
                    b = next(t, b);
                }
                if b <= le && b > p {
                    let rep: String = std::iter::repeat_n(c, n).collect();
                    t.replace_range(p..b, &rep);
                    fx.changed = true;
                    *cursor = p + rep.len() - c.len_utf8();
                }
            }
            Action::Put(after) => {
                let reg = self.register.clone();
                if reg.text.is_empty() {
                    return;
                }
                let body = reg.text.repeat(n);
                if reg.linewise {
                    let body = body.strip_suffix('\n').unwrap_or(&body).to_string();
                    let at = if after {
                        let le = line_end(t, p);
                        t.insert_str(le, &format!("\n{body}"));
                        le + 1
                    } else {
                        let ls = line_start(t, p);
                        t.insert_str(ls, &format!("{body}\n"));
                        ls
                    };
                    *cursor = first_nonblank(t, at);
                } else {
                    let at = if after && line_end(t, p) > p {
                        next(t, p)
                    } else {
                        p
                    };
                    t.insert_str(at, &body);
                    *cursor = prev(t, at + body.len()).max(at);
                }
                fx.changed = true;
            }
            Action::Join => {
                for _ in 0..n.max(1) {
                    let le = line_end(t, *cursor);
                    if le >= t.len() {
                        break;
                    }
                    let next_start = le + 1;
                    let fnb = first_nonblank(t, next_start);
                    let next_empty = line_end(t, next_start) == fnb;
                    let sep = if next_empty || t[..le].ends_with(' ') {
                        ""
                    } else {
                        " "
                    };
                    t.replace_range(le..fnb, sep);
                    fx.changed = true;
                    *cursor = le;
                }
            }
            Action::ToggleCase => {
                let le = line_end(t, p);
                let mut e = p;
                for _ in 0..n {
                    if e < le {
                        e = next(t, e);
                    }
                }
                let toggled = toggle_case(&t[p..e]);
                if toggled != t[p..e] {
                    t.replace_range(p..e, &toggled);
                    fx.changed = true;
                }
                *cursor = clamp_normal(t, p + toggled.len());
            }
            Action::Visual(line) => {
                let mode = if line { Mode::VisualLine } else { Mode::Visual };
                if self.mode == mode {
                    self.mode = Mode::Normal;
                } else {
                    if !visual {
                        self.anchor = p;
                    }
                    self.mode = mode;
                }
            }
            Action::Repeat => {
                let keys = self.last_change.clone();
                if keys.is_empty() {
                    return;
                }
                self.replaying = true;
                for _ in 0..n {
                    for &k in &keys {
                        let r = self.handle(t, cursor, k);
                        fx.changed |= r.changed;
                    }
                    if self.mode == Mode::Insert {
                        let r = self.handle(t, cursor, Key::Esc);
                        fx.changed |= r.changed;
                    }
                }
                self.replaying = false;
            }
            Action::Undo => fx.cmd = Some(Cmd::Undo(n)),
            Action::Find(fwd) => fx.cmd = Some(Cmd::OpenFind(fwd)),
            Action::FindNext(fwd) => fx.cmd = Some(if fwd { Cmd::FindNext } else { Cmd::FindPrev }),
            Action::SearchWord(fwd) => {
                if let Some((a, b)) = word_object(t, p, false, false) {
                    let w = t[a..b].trim();
                    if !w.is_empty() {
                        fx.cmd = Some(Cmd::SearchWord(w.to_string(), fwd));
                    }
                }
            }
            Action::Cmdline => self.cmdline = Some(String::new()),
        }
    }

    fn yank_text(&mut self, s: &str, linewise: bool, fx: &mut Effects) {
        let mut text = s.to_string();
        if linewise && !text.ends_with('\n') {
            text.push('\n');
        }
        self.register = Register {
            text: text.clone(),
            linewise,
        };
        fx.yank = Some(text);
    }

    /// 演算子を範囲に適用する。`b` は含まない。行単位なら a..b は行の内容（改行を含まない）。
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &mut self,
        op: Op,
        a: usize,
        b: usize,
        linewise: bool,
        t: &mut String,
        cursor: &mut usize,
        fx: &mut Effects,
    ) {
        let (a, b) = (a.min(t.len()), b.min(t.len()));
        match op {
            Op::Yank => {
                self.yank_text(&t[a..b], linewise, fx);
                *cursor = a;
            }
            Op::Delete => {
                self.yank_text(&t[a..b], linewise, fx);
                if linewise {
                    let (s, e) = linewise_range(t, a, b.max(a));
                    t.replace_range(s..e, "");
                    *cursor = first_nonblank(
                        t,
                        s.min(t.len())
                            .max(if s > 0 && s == t.len() {
                                line_start(t, s)
                            } else {
                                0
                            })
                            .min(t.len()),
                    );
                    if s >= t.len() {
                        *cursor = first_nonblank(t, line_start(t, t.len()));
                    }
                } else {
                    t.replace_range(a..b, "");
                    *cursor = clamp_normal(t, a);
                }
                fx.changed = true;
            }
            Op::Change => {
                self.yank_text(&t[a..b], linewise, fx);
                let start = if linewise { first_nonblank(t, a) } else { a };
                let end = if linewise { line_end(t, b) } else { b };
                t.replace_range(start..end, "");
                fx.changed |= end > start;
                *cursor = start;
                self.enter_insert(t, start);
            }
            Op::Indent | Op::Outdent => {
                let unit = indent_unit(t);
                let (s, e) = (line_start(t, a), line_end(t, b.max(a)));
                let changed: Vec<String> = t[s..e]
                    .split('\n')
                    .map(|line| {
                        if op == Op::Indent {
                            if line.is_empty() {
                                String::new()
                            } else {
                                format!("{unit}{line}")
                            }
                        } else if let Some(rest) = line.strip_prefix(&unit) {
                            rest.to_string()
                        } else {
                            line.trim_start_matches([' ', '\t']).to_string()
                        }
                    })
                    .collect();
                let joined = changed.join("\n");
                if joined != t[s..e] {
                    t.replace_range(s..e, &joined);
                    fx.changed = true;
                }
                *cursor = first_nonblank(t, s);
            }
            Op::Lower | Op::Upper | Op::Toggle => {
                let src = &t[a..b];
                let out = match op {
                    Op::Lower => src.to_lowercase(),
                    Op::Upper => src.to_uppercase(),
                    _ => toggle_case(src),
                };
                if out != src {
                    t.replace_range(a..b, &out);
                    fx.changed = true;
                }
                *cursor = clamp_normal(t, a);
            }
        }
    }

    fn visual_op(&mut self, op: Op, t: &mut String, cursor: &mut usize, fx: &mut Effects) {
        let Some((a, b)) = self.selection(t, *cursor) else {
            return;
        };
        let linewise = self.mode == Mode::VisualLine;
        self.mode = Mode::Normal;
        self.apply(op, a, b, linewise, t, cursor, fx);
    }

    /// `:` コマンド。
    fn ex(&mut self, line: &str, t: &str, cursor: &mut usize, fx: &mut Effects) {
        fx.cmd = Some(match line {
            "w" | "w!" => Cmd::Save,
            "q" | "q!" => Cmd::Close,
            "wq" | "x" | "wq!" => Cmd::SaveClose,
            "noh" | "nohl" | "nohlsearch" => Cmd::ClearSearch,
            "" => return,
            n if n.chars().all(|c| c.is_ascii_digit()) => {
                let n: usize = n.parse().unwrap_or(1);
                *cursor = first_nonblank(t, nth_line(t, n.saturating_sub(1)));
                return;
            }
            other => Cmd::Unknown(other.to_string()),
        });
    }
}

fn toggle_case(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_lowercase() {
                c.to_uppercase().collect::<Vec<_>>()
            } else {
                c.to_lowercase().collect::<Vec<_>>()
            }
        })
        .collect()
}

/// 全角英数字や記号を半角にする（IMEがオンのままでもノーマルモードのキーとして使えるように）。
pub fn normalize_key(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        '\u{3000}' => ' ',
        '。' => '.',
        '、' => ',',
        '・' => '/',
        'ー' => '-',
        '「' => '[',
        '」' => ']',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `|` をカーソルにしたテキストに、キー列（`<esc>` `<cr>` `<bs>` `<c-r>` が使える）を送る。
    fn run(marked: &str, keys: &str) -> (String, Vim) {
        let mut vim = Vim::default();
        let (out, _) = feed(&mut vim, marked, keys);
        (out, vim)
    }

    fn feed(vim: &mut Vim, marked: &str, keys: &str) -> (String, Vec<Effects>) {
        let mut cursor = marked.find('|').expect("cursor");
        let mut text = marked.replacen('|', "", 1);
        let mut all = Vec::new();
        let mut rest = keys;
        while !rest.is_empty() {
            let (key, len) = if let Some(r) = rest.strip_prefix("<esc>") {
                let _ = r;
                (Key::Esc, 5)
            } else if rest.starts_with("<cr>") {
                (Key::Enter, 4)
            } else if rest.starts_with("<bs>") {
                (Key::Backspace, 4)
            } else if rest.starts_with("<c-r>") {
                (Key::CtrlR, 5)
            } else {
                let c = rest.chars().next().unwrap();
                (Key::Char(c), c.len_utf8())
            };
            rest = &rest[len..];
            // 挿入モードの文字は、実際にはエディタが入れる（ここで真似する）
            if vim.mode == Mode::Insert && vim.cmdline.is_none() {
                if let Key::Char(c) = key {
                    text.insert(cursor, c);
                    cursor += c.len_utf8();
                    continue;
                }
                if key == Key::Enter {
                    text.insert(cursor, '\n');
                    cursor += 1;
                    continue;
                }
            }
            all.push(vim.handle(&mut text, &mut cursor, key));
        }
        (format!("{}|{}", &text[..cursor], &text[cursor..]), all)
    }

    fn t(marked: &str, keys: &str) -> String {
        run(marked, keys).0
    }

    #[test]
    fn motions() {
        assert_eq!(t("|hello world", "w"), "hello |world");
        assert_eq!(t("|hello world", "e"), "hell|o world");
        assert_eq!(t("hello |world", "b"), "|hello world");
        assert_eq!(t("|a.b c", "w"), "a|.b c");
        assert_eq!(t("|a.b c", "W"), "a.b |c");
        assert_eq!(t("|one two three", "2w"), "one two |three");
        assert_eq!(t("ab|c\nd", "l"), "ab|c\nd");
        assert_eq!(t("a|bc", "$"), "ab|c");
        assert_eq!(t("  a|bc", "0"), "|  abc");
        assert_eq!(t("  a|bc", "^"), "  |abc");
        assert_eq!(t("abc\nd|e\nfgh", "gg"), "|abc\nde\nfgh");
        assert_eq!(t("a|bc\nde\nfgh", "G"), "abc\nde\n|fgh");
        assert_eq!(t("a|bc\nde\nfgh", "2G"), "abc\n|de\nfgh");
        assert_eq!(t("|a,b,c", "f,;"), "a,b|,c");
        assert_eq!(t("|a,b,c", "t,"), "|a,b,c");
        assert_eq!(t("|ab,c", "t,"), "a|b,c");
        assert_eq!(t("|a,b,c", "t,;"), "a,|b,c");
        assert_eq!(t("a,b,|c", "F,"), "a,b|,c");
        assert_eq!(t("|f(a[b]c)", "%"), "f(a[b]c|)");
        // 上下移動は列を覚えておく
        assert_eq!(t("abc|d\nx\nabcd", "jj"), "abcd\nx\nabc|d");
        assert_eq!(t("|p1\np1\n\np2", "}"), "p1\np1\n|\np2");
        // 日本語は文字の種類で区切る
        assert_eq!(t("|日本語のテキスト", "w"), "日本語|のテキスト");
        assert_eq!(t("|日本語のテキスト", "ww"), "日本語の|テキスト");
        assert_eq!(t("abc\n|\ndef", "k"), "|abc\n\ndef");
    }

    #[test]
    fn operators() {
        assert_eq!(t("|hello world", "dw"), "|world");
        assert_eq!(t("hello |world\nnext", "dw"), "hello| \nnext");
        assert_eq!(t("|hello world", "cwhi<esc>"), "h|i world");
        assert_eq!(t("|a b c d", "d2w"), "|c d");
        assert_eq!(t("|a b c d", "2dw"), "|c d");
        assert_eq!(t("a|bc def", "d$"), "|a");
        assert_eq!(t("a|bc def", "D"), "|a");
        assert_eq!(t("one\n|two\nthree", "dd"), "one\n|three");
        assert_eq!(t("one\ntwo\n|three", "dd"), "one\n|two");
        assert_eq!(t("|one\ntwo\nthree", "2dd"), "|three");
        assert_eq!(t("|one\ntwo", "dj"), "|");
        assert_eq!(t("  |one\ntwo", "cc"), "  |\ntwo");
        assert_eq!(t("f(a, |b)", "di("), "f(|)");
        assert_eq!(t("f(a, |b)", "da("), "|f");
        assert_eq!(t("say \"hi |there\" ok", "ci\"yo<esc>"), "say \"y|o\" ok");
        assert_eq!(t("foo b|ar baz", "diw"), "foo | baz");
        assert_eq!(t("foo b|ar baz", "daw"), "foo |baz");
        assert_eq!(t("p1\n|p1\n\np2", "dap"), "|p2");
        assert_eq!(t("|a\nb", ">>"), "\t|a\nb");
        assert_eq!(t("\t|a", "<<"), "|a");
        assert_eq!(t("|hello", "gUiw"), "|HELLO");
        assert_eq!(t("|Hello", "g~~"), "|hELLO");
    }

    #[test]
    fn edits() {
        assert_eq!(t("a|bc", "x"), "a|c");
        assert_eq!(t("ab|c", "x"), "a|b");
        assert_eq!(t("a|bcd", "2x"), "a|d");
        assert_eq!(t("ab|c", "X"), "a|c");
        assert_eq!(t("a|bc", "rx"), "a|xc");
        assert_eq!(t("|abc", "~~"), "AB|c");
        assert_eq!(t("|a\n  b", "J"), "a| b");
        assert_eq!(t("|abc", "ix<esc>"), "|xabc");
        assert_eq!(t("|abc", "ax<esc>"), "a|xbc");
        assert_eq!(t("a|bc", "Ax<esc>"), "abc|x");
        assert_eq!(t("  a|bc", "Ix<esc>"), "  |xabc");
        assert_eq!(t("  |a", "ob<esc>"), "  a\n  |b");
        assert_eq!(t("|a", "Ob<esc>"), "|b\na");
        assert_eq!(t("a|bc", "sX<esc>"), "a|Xc");
        assert_eq!(t("a|bc", "Cz<esc>"), "a|z");
    }

    #[test]
    fn yank_put_and_repeat() {
        assert_eq!(t("|ab cd", "ywP"), "ab| ab cd");
        assert_eq!(t("|one\ntwo", "yyp"), "one\n|one\ntwo");
        assert_eq!(t("|one\ntwo", "ddp"), "two\n|one");
        assert_eq!(t("a|bc", "xp"), "ac|b");
        assert_eq!(t("|a b c d", "dw.."), "|d");
        assert_eq!(t("|x x x", "cwy<esc>w.w."), "y y |y");
        assert_eq!(t("|a\nb\nc", "A;<esc>j.j."), "a;\nb;\nc|;");
        let (_, fx) = feed(&mut Vim::default(), "|ab", "yl");
        assert_eq!(fx.last().unwrap().yank.as_deref(), Some("a"));
    }

    #[test]
    fn visual() {
        assert_eq!(t("|abc def", "vld"), "|c def");
        assert_eq!(t("|abc def", "vey"), "|abc def");
        assert_eq!(t("a\n|b\nc\nd", "Vjd"), "a\n|d");
        assert_eq!(t("|ab", "vU").replace('|', ""), "Ab");
        let (_, vim) = run("|abc", "vl");
        assert_eq!(vim.mode, Mode::Visual);
        assert_eq!(vim.selection("abc", 1), Some((0, 2)));
        let (_, vim) = run("|abc", "vl<esc>");
        assert_eq!(vim.mode, Mode::Normal);
    }

    #[test]
    fn commands() {
        let cmd = |keys: &str| {
            let (_, fx) = feed(&mut Vim::default(), "|ab cd\nef", keys);
            fx.into_iter().filter_map(|f| f.cmd).collect::<Vec<_>>()
        };
        assert_eq!(cmd(":w<cr>"), [Cmd::Save]);
        assert_eq!(cmd(":wq<cr>"), [Cmd::SaveClose]);
        assert_eq!(cmd(":q<cr>"), [Cmd::Close]);
        assert_eq!(cmd(":foo<cr>"), [Cmd::Unknown("foo".into())]);
        assert_eq!(cmd(":x<bs><esc>"), []);
        assert_eq!(cmd("u"), [Cmd::Undo(1)]);
        assert_eq!(cmd("3u"), [Cmd::Undo(3)]);
        assert_eq!(cmd("<c-r>"), [Cmd::Redo(1)]);
        assert_eq!(cmd("/"), [Cmd::OpenFind(true)]);
        assert_eq!(cmd("n"), [Cmd::FindNext]);
        assert_eq!(cmd("*"), [Cmd::SearchWord("ab".into(), true)]);
        assert_eq!(t("|a\nb\nc", ":3<cr>"), "a\nb\n|c");
        // 不完全なキー列は保留、無効なキー列は捨てる
        let (_, vim) = run("|abc", "d");
        assert_eq!(vim.pending(), "d");
        let (_, vim) = run("|abc", "dz");
        assert_eq!(vim.pending(), "");
        assert_eq!(normalize_key('ｊ'), 'j');
        assert_eq!(normalize_key('：'), ':');
    }
}
